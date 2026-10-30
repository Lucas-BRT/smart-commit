#!/usr/bin/env bash
# The installer's pre-flight checks (git, glibc, free disk space) against a
# fake release (file:// URLs), a fake os-release, and fake getconf, df and
# git commands in a temp PATH. No network, sudo or Docker.
#
#   cli/tests/scripts/test-install-preflight.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
failures=0

stage=$work/stage/smart-commit-x86_64-unknown-linux-gnu
mkdir -p "$stage/bin" "$stage/share/smart-commit/scripts" "$stage/share/doc/smart-commit" "$work/releases/latest/download"
printf '#!/bin/sh\necho "smart-commit 1.0.0"\n' >"$stage/bin/smart-commit"
chmod +x "$stage/bin/smart-commit"
echo 1.0.0 >"$stage/share/smart-commit/VERSION"
echo licence >"$stage/share/doc/smart-commit/LICENSE"
tar -C "$work/stage" -czf "$work/releases/latest/download/$ASSET" smart-commit-x86_64-unknown-linux-gnu
(cd "$work/releases/latest/download" && sha256sum "$ASSET" >SHA256SUMS)

# A PATH of symlinks to the real tools the installer uses, without git; the
# fake git, getconf and df are added per case.
bin=$work/bin
mkdir -p "$bin"
for tool in bash sh curl tar sha256sum uname mktemp rm mkdir cat cp install rmdir grep sed awk sort head dirname gzip; do
    path=$(command -v "$tool" || true)
    [ -n "$path" ] && ln -sf "$path" "$bin/$tool"
done
printf '#!/bin/sh\nexit 0\n' >"$work/fake-git"
chmod +x "$work/fake-git"

home=$work/home
mkdir -p "$home"
# run ARGS...: GIT (yes|no), GLIBC (the getconf output), FREE (KiB), OSR
# (os-release contents) and KEEP (keep the home) are set by the caller.
run() {
    rm -f "$bin/git" "$bin/df" "$bin/getconf"
    [ "${GIT:-yes}" = yes ] && cp "$work/fake-git" "$bin/git"
    printf '#!/bin/sh\necho "Filesystem 1024-blocks Used Available Capacity Mounted\n/dev/x 9 9 %s 1%% /"\n' "${FREE:-9000000}" >"$bin/df"
    printf '#!/bin/sh\n[ -n "%s" ] || exit 1\necho "%s"\n' "${GLIBC-glibc 2.39}" "${GLIBC-glibc 2.39}" >"$bin/getconf"
    chmod +x "$bin/df" "$bin/getconf"
    printf '%s\n' "${OSR-ID=ubuntu}" >"$work/os-release"
    [ -n "${KEEP:-}" ] || rm -rf "$home/.local"
    HOME=$home XDG_DATA_HOME=$home/.local/share XDG_CACHE_HOME=$home/.cache XDG_CONFIG_HOME=$home/.config \
        SMART_COMMIT_RELEASE_BASE=file://$work/releases SMART_COMMIT_OS_RELEASE=$work/os-release \
        SMART_COMMIT_GETCONF=$bin/getconf PATH=$bin \
        bash "$ROOT/install.sh" "$@" </dev/null >"$work/out" 2>&1
}

check() {
    local name=$1
    shift
    if "$@"; then
        echo "ok    $name"
    else
        echo "FAIL  $name"
        sed 's/^/        /' "$work/out"
        failures=$((failures + 1))
    fi
}
fails() { ! run "$@"; }
said() { grep -qF -- "$1" "$work/out"; }
not() { ! "$@"; }
installed() { [ -x "$home/.local/bin/smart-commit" ]; }

# git
git_case() {
    local name=$1 osr=$2 want=$3
    GIT=no OSR=$osr fails --no-setup
    check "no git on $name: fails" said "git is required"
    check "no git on $name: hint" said "$want"
    check "no git on $name: installs nothing" not installed
}
git_case debian "ID=debian" "sudo apt install git"
git_case "ubuntu-like" $'ID=linuxmint\nID_LIKE="ubuntu debian"' "sudo apt install git"
git_case fedora "ID=fedora" "sudo dnf install git"
git_case rhel-like $'ID=rocky\nID_LIKE="rhel centos fedora"' "sudo dnf install git"
git_case arch "ID=arch" "sudo pacman -S git"
git_case opensuse $'ID="opensuse-leap"\nID_LIKE="suse opensuse"' "sudo zypper install git"
git_case unknown "ID=plan9" "install git"
GIT=no OSR=ID=plan9 fails --no-setup
check "unknown distribution names no package manager" not said "sudo "
GIT=no OSR='' fails --no-setup
check "no os-release: still says git is required" said "git is required"

# An old version is there: --update of a first install ends with status 1.
GIT=yes run --no-setup
mkdir -p "$home/.local/share/smart-commit"
echo 0.9.0 >"$home/.local/share/smart-commit/VERSION"
KEEP=1 GIT=no run --update
check "--update without git installs" installed
check "--update without git warns" said "warning: git is not installed"
GIT=no run --uninstall
check "--uninstall skips the checks" not said "git is required"

# glibc
GLIBC="glibc 2.35" run --no-setup
check "glibc 2.35 is enough" installed
GLIBC="glibc 2.39" run --no-setup
check "glibc 2.39 is enough" installed
GLIBC="glibc 2.9" fails --no-setup
check "glibc 2.9 fails (version order, not text order)" said "this system has glibc 2.9"
GLIBC="glibc 2.34" fails --no-setup
check "glibc 2.34 fails and says the version" said "this system has glibc 2.34"
check "glibc 2.34 installs nothing" not installed
GLIBC="" fails --no-setup
check "no glibc (musl) fails" said "does not use glibc"
GLIBC="glibc 2.34" run --uninstall
check "--uninstall ignores an old glibc" not said "needs glibc"

# disk space
FREE=100000 run
check "low disk: carries on" installed
check "low disk: warns" said "warning: only 97 MB free"
FREE=9000000 run
check "enough disk: silent" not said "warning:"
FREE=100000 run --no-setup
check "--no-setup does not check the disk" not said "warning:"
FREE=100000 run --update
check "--update does not check the disk" not said "warning:"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all cases passed"

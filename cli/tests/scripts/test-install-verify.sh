#!/usr/bin/env bash
# install.sh --verify against a fake release (file:// URLs) and a fake `gh`
# that accepts, rejects, finds no attestation, or is missing. No network.
#
#   cli/tests/scripts/test-install-verify.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
failures=0
SETSID=$(command -v setsid)

stage=$work/stage/smart-commit-x86_64-unknown-linux-gnu
mkdir -p "$stage/bin" "$stage/share/smart-commit/scripts" "$stage/share/doc/smart-commit"
printf '#!/bin/sh\necho "smart-commit 1.0.0"\n' >"$stage/bin/smart-commit"
chmod +x "$stage/bin/smart-commit"
echo 1.0.0 >"$stage/share/smart-commit/VERSION"
echo licence >"$stage/share/doc/smart-commit/LICENSE"
mkdir -p "$work/releases/latest/download"
tar -C "$work/stage" -czf "$work/releases/latest/download/$ASSET" smart-commit-x86_64-unknown-linux-gnu
(cd "$work/releases/latest/download" && sha256sum "$ASSET" >SHA256SUMS)

home=$work/home
prefix=$home/.local

# fake_gh MODE: a gh that records its arguments and behaves as MODE.
fake_gh() {
    mkdir -p "$work/bin"
    cat >"$work/bin/gh" <<GH
#!/bin/sh
echo "\$*" >"$work/gh-args"
case $1 in
    accept) echo "Loaded digest sha256:abc"; exit 0 ;;
    reject) echo "verification failed: signature mismatch" >&2; exit 1 ;;
    none) echo "no attestations found for subject" >&2; exit 1 ;;
esac
GH
    chmod +x "$work/bin/gh"
}

# Only the tools install.sh needs, so that gh can be absent.
mkdir -p "$work/tools"
for tool in bash curl tar gzip sha256sum mktemp rm mkdir cp install cat grep sed head dirname uname sort tr date id ls mv ln chmod find readlink basename touch; do
    path=$(command -v "$tool" 2>/dev/null || true)
    [ -n "$path" ] && ln -sf "$path" "$work/tools/$tool"
done

# run PATH_DIRS ARGS...: install.sh with a PATH of only those directories.
run() {
    local path=$1
    shift
    rm -rf "$prefix" "$work/gh-args"
    HOME=$home XDG_DATA_HOME=$prefix/share XDG_CACHE_HOME=$home/.cache XDG_CONFIG_HOME=$home/.config \
        SMART_COMMIT_RELEASE_BASE=file://$work/releases PATH=$path \
        "$SETSID" "$BASH" "$ROOT/install.sh" --no-setup "$@" </dev/null >"$work/out" 2>&1
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
not() { ! "$@"; }
installed() { [ -x "$prefix/bin/smart-commit" ]; }
said() { grep -qF -- "$1" "$work/out"; }

fake_gh accept
run "$work/bin:$work/tools" --verify
check "--verify installs when gh accepts" installed
check "--verify asks gh about the download and the repository" \
    grep -Eq "^attestation verify .*$ASSET --repo Lucas-BRT/smart-commit$" "$work/gh-args"

fake_gh reject
check "--verify fails when gh rejects" not run "$work/bin:$work/tools" --verify
check "a rejected download is not installed" not installed
check "a rejection says so" said "could not be verified"

fake_gh none
check "--verify fails without an attestation" not run "$work/bin:$work/tools" --verify
check "an old release gets a plain message" said "no attestation: releases up to 0.14.0 have none"
check "an old release is not installed" not installed

check "--verify without gh fails" not run "$work/tools" --verify
check "...and says where to get it" said "--verify needs the GitHub CLI: https://cli.github.com"

fake_gh reject
run "$work/bin:$work/tools"
check "without --verify, gh is never run" not test -e "$work/gh-args"
check "without --verify, the install works" installed

[ "$failures" = 0 ] || {
    echo "$failures check(s) failed" >&2
    exit 1
}
echo "all checks passed"

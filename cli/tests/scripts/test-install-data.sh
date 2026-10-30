#!/usr/bin/env bash
# Installs, upgrades and uninstalls a fake release (file:// URLs) with the
# default layout, where the release's share/smart-commit is also the data
# directory: the models, llama.cpp and the benchmark there must survive all
# of it, unless --purge asks otherwise. --system runs against a fake root.
# No network, sudo or Docker.
#
#   cli/tests/scripts/test-install-data.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
failures=0

# release VERSION: a release with a stub binary, in GitHub's layout.
release() {
    local stage=$work/stage/smart-commit-x86_64-unknown-linux-gnu
    rm -rf "$work/stage"
    mkdir -p "$stage/bin" "$stage/share/smart-commit/scripts" "$stage/share/doc/smart-commit"
    # Records how --update calls its setup.
    # shellcheck disable=SC2016  # the $1 and $* are for the stub, not for this shell
    printf '#!/bin/sh\nif [ "$1" = setup ]; then echo "%s $*" >>"%s/llamacpp-calls"; exit 0; fi\necho "smart-commit %s"\n' \
        "$1" "$work" "$1" >"$stage/bin/smart-commit"
    chmod +x "$stage/bin/smart-commit"
    echo "# setup $1" >"$stage/share/smart-commit/scripts/setup-gpu.sh"
    cp "$ROOT/scripts/setup-llamacpp.sh" "$stage/share/smart-commit/scripts/"
    echo "$1" >"$stage/share/smart-commit/VERSION"
    echo licence >"$stage/share/doc/smart-commit/LICENSE"
    mkdir -p "$work/releases/latest/download"
    tar -C "$work/stage" -czf "$work/releases/latest/download/$ASSET" smart-commit-x86_64-unknown-linux-gnu
    cp "$ROOT/install.sh" "$work/releases/latest/download/install.sh"
    (cd "$work/releases/latest/download" && sha256sum "$ASSET" install.sh >SHA256SUMS)
}

home=$work/home
prefix=$home/.local
data=$prefix/share/smart-commit

root=$work/root
# setsid: no controlling terminal, so questions are never asked, even when
# the test runs in one.
run() {
    HOME=$home XDG_DATA_HOME=$prefix/share XDG_CACHE_HOME=$home/.cache XDG_CONFIG_HOME=$home/.config \
        SMART_COMMIT_RELEASE_BASE=file://$work/releases SMART_COMMIT_SYSTEM_ROOT=$root SMART_COMMIT_SUDO='' \
        setsid bash "$ROOT/install.sh" "$@" </dev/null >"$work/out" 2>&1
}

# check NAME COMMAND...: passes when COMMAND succeeds.
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

# What the setup leaves in the data directory.
setup_data() {
    mkdir -p "$data/models/m" "$data/llama.cpp/llama-b11381"
    echo weights >"$data/models/m/model.bin"
    echo server >"$data/llama.cpp/llama-b11381/llama-server"
    echo '{}' >"$data/bench.json"
}

installed() { [ "$("$prefix/bin/smart-commit")" = "smart-commit $1" ]; }
upgraded() { [ "$(cat "$data/VERSION")" = "$1" ] && grep -q "setup $1" "$data/scripts/setup-gpu.sh"; }
release_gone() {
    [ ! -e "$prefix/bin/smart-commit" ] && [ ! -e "$data/scripts" ] && [ ! -e "$data/VERSION" ] &&
        [ ! -e "$prefix/share/doc/smart-commit" ]
}
absent() { [ ! -e "$1" ]; }
not() { ! "$@"; }

data_kept() {
    [ -f "$data/models/m/model.bin" ] && [ -f "$data/llama.cpp/llama-b11381/llama-server" ] &&
        [ -f "$data/bench.json" ]
}

release 1.0.0
run --no-setup
check "installs" installed 1.0.0
setup_data

release 1.1.0
run --no-setup
check "upgrades" upgraded 1.1.0
check "an upgrade keeps the models, llama.cpp and the benchmark" data_kept

# --no-setup says what to run next, with the full path when the prefix is
# not on PATH.
check "--no-setup prints the full path off PATH" grep -qF "Next: run \`$prefix/bin/smart-commit setup\`" "$work/out"
PATH=$prefix/bin:$PATH run --no-setup
check "--no-setup prints the bare command on PATH" grep -qxF "Next: run \`smart-commit setup\` to set up the devices and models." "$work/out"

# --update: only a different version is installed, without the setup, and
# llama.cpp follows the build of the new release.
llamacpp_calls() { [ "$(cat "$work/llamacpp-calls" 2>/dev/null)" = "$1" ]; }
run --update
check "--update keeps the same version" grep -qF "smart-commit 1.1.0 is already the latest release" "$work/out"
check "--update of the same version runs nothing" llamacpp_calls ""
release 1.2.0
run --update
check "--update installs a new version" upgraded 1.2.0
check "--update says from what" grep -qF "updated from 1.1.0" "$work/out"
check "--update keeps the models, llama.cpp and the benchmark" data_kept
check "--update brings llama.cpp to the new release's build" llamacpp_calls "1.2.0 setup --llamacpp-only"
check "--update does not point to the setup" not grep -qF "smart-commit setup" "$work/out"
# What the install.sh of 0.7.0 and 0.8.0 does after an update: it runs the
# new release's setup-llamacpp.sh, which must still be there.
rm -f "$work/llamacpp-calls"
check "the old installers' setup-llamacpp.sh still brings llama.cpp" \
    bash -c "'$data/scripts/setup-llamacpp.sh' --server-only"
check "...through the binary" llamacpp_calls "1.2.0 setup --llamacpp-only"
rm -f "$work/llamacpp-calls"
mv "$data/llama.cpp" "$work/llama.cpp.aside"
release 1.3.0
run --update
check "--update without llama.cpp installs none" llamacpp_calls ""
mv "$work/llama.cpp.aside" "$data/llama.cpp"

run --uninstall
check "uninstall removes the binary, scripts and docs" release_gone
check "uninstall keeps the models, llama.cpp and the benchmark" data_kept
check "uninstall says what it kept" grep -qF "models and benchmark: $data" "$work/out"

# Without data, nothing is left behind.
rm -rf "$data"
run --no-setup
run --uninstall
check "uninstall leaves no empty directory" absent "$data"

# --purge: the data, the caches and the configuration go too.
run --no-setup
setup_data
mkdir -p "$home/.cache/smart-commit/openvino" "$home/.config/smart-commit"
echo 'backend = "auto"' >"$home/.config/smart-commit/config.toml"
run --purge
purged() {
    release_gone && [ ! -e "$data" ] && [ ! -e "$home/.cache/smart-commit" ] &&
        [ ! -e "$home/.config/smart-commit" ]
}
check "--purge removes the data, caches and configuration" purged
check "--purge leaves the rest of the home alone" test -d "$home/.config"

# openvino_install MARKED: what setup-npu.sh leaves, with its ld.so.conf.d
# file when MARKED is yes.
openvino_install() {
    mkdir -p "$root/opt/intel/openvino_2026.3.1/runtime" "$root/etc/ld.so.conf.d"
    ln -sfn /opt/intel/openvino_2026.3.1 "$root/opt/intel/openvino"
    echo "other" >"$root/etc/ld.so.conf.d/other.conf"
    if [ "$1" = yes ]; then
        echo /opt/intel/openvino/runtime/lib/intel64 >"$root/etc/ld.so.conf.d/smart-commit-openvino.conf"
    fi
}
openvino_gone() {
    [ ! -e "$root/opt/intel" ] && [ ! -e "$root/etc/ld.so.conf.d/smart-commit-openvino.conf" ] &&
        [ -f "$root/etc/ld.so.conf.d/other.conf" ]
}
openvino_kept() { [ -d "$root/opt/intel/openvino_2026.3.1" ] && [ -L "$root/opt/intel/openvino" ]; }

# --system without --yes and without a terminal asks nothing and keeps it.
openvino_install yes
run --no-setup
run --system
check "--system without a terminal keeps OpenVINO" openvino_kept
check "--system without a terminal says to pass --yes" grep -qF "[skipped: no terminal; pass --yes]" "$work/out"
check "--system still uninstalls" release_gone

run --no-setup
setup_data
run --system --yes
check "--system --yes removes setup's OpenVINO" openvino_gone
check "--system lists what it removes" grep -qF "$root/opt/intel/openvino_2026.3.1" "$work/out"
check "--system keeps the driver packages" grep -qF "Kept: driver packages" "$work/out"
check "--system without --purge keeps the data" data_kept

# An OpenVINO that smart-commit did not install is never touched.
rm -rf "$root"
openvino_install no
run --no-setup
run --system --yes
check "--system keeps an OpenVINO it did not install" openvino_kept
check "--system says there was nothing of its own" grep -qF "no OpenVINO installed by smart-commit" "$work/out"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all cases passed"

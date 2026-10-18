#!/usr/bin/env bash
# Installs, upgrades and uninstalls a fake release (file:// URLs) with the
# default layout, where the release's share/smart-commit is also the data
# directory: the models, llama.cpp and the benchmark there must survive all
# of it. No network, sudo or Docker.
#
#   tests/scripts/test-install-data.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
failures=0

# release VERSION: a release with a stub binary, in GitHub's layout.
release() {
    local stage=$work/stage/smart-commit-x86_64-unknown-linux-gnu
    rm -rf "$work/stage"
    mkdir -p "$stage/bin" "$stage/share/smart-commit/scripts" "$stage/share/doc/smart-commit"
    printf '#!/bin/sh\necho "smart-commit %s"\n' "$1" >"$stage/bin/smart-commit"
    chmod +x "$stage/bin/smart-commit"
    echo "# setup $1" >"$stage/share/smart-commit/scripts/setup.sh"
    echo "$1" >"$stage/share/smart-commit/VERSION"
    echo licence >"$stage/share/doc/smart-commit/LICENSE"
    mkdir -p "$work/releases/latest/download"
    tar -C "$work/stage" -czf "$work/releases/latest/download/$ASSET" smart-commit-x86_64-unknown-linux-gnu
    (cd "$work/releases/latest/download" && sha256sum "$ASSET" >SHA256SUMS)
}

home=$work/home
prefix=$home/.local
data=$prefix/share/smart-commit

run() {
    HOME=$home XDG_DATA_HOME=$prefix/share XDG_CACHE_HOME=$home/.cache XDG_CONFIG_HOME=$home/.config \
        SMART_COMMIT_RELEASE_BASE=file://$work/releases bash "$ROOT/install.sh" "$@" >"$work/out" 2>&1
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
upgraded() { [ "$(cat "$data/VERSION")" = "$1" ] && grep -q "setup $1" "$data/scripts/setup.sh"; }
release_gone() {
    [ ! -e "$prefix/bin/smart-commit" ] && [ ! -e "$data/scripts" ] && [ ! -e "$data/VERSION" ] &&
        [ ! -e "$prefix/share/doc/smart-commit" ]
}
absent() { [ ! -e "$1" ]; }

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

run --uninstall
check "uninstall removes the binary, scripts and docs" release_gone
check "uninstall keeps the models, llama.cpp and the benchmark" data_kept
check "uninstall says what it kept" grep -qF "models and benchmark: $data" "$work/out"

# Without data, nothing is left behind.
rm -rf "$data"
run --no-setup
run --uninstall
check "uninstall leaves no empty directory" absent "$data"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all cases passed"

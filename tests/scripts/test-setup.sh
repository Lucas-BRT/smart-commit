#!/usr/bin/env bash
# Runs scripts/setup.sh without drivers against a fake /sys and a fake
# release on disk, with a stub smart-commit: it reports the hardware, installs
# the model and runs the benchmark. No network, sudo or hardware.
#
#   tests/scripts/test-setup.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
NAME=qwen2.5-coder-1.5b-instruct-int4-sym-g128

# The installed layout: prefix/bin and prefix/share/smart-commit/scripts.
prefix=$work/prefix
mkdir -p "$prefix/bin" "$prefix/share/smart-commit/scripts/export"
cp "$ROOT"/scripts/{setup.sh,setup-npu.sh,setup-gpu.sh,check-npu.sh,fetch-model.sh} "$prefix/share/smart-commit/scripts/"
printf '#!/bin/sh\necho "stub smart-commit $*"\n' >"$prefix/bin/smart-commit"
chmod +x "$prefix/bin/smart-commit"

# A fake model, its manifest and its release.
mkdir -p "$work/model/$NAME"
echo weights >"$work/model/$NAME/openvino_model.bin"
(cd "$work/model/$NAME" && sha256sum openvino_model.bin >MANIFEST.sha256)
cp "$work/model/$NAME/MANIFEST.sha256" "$prefix/share/smart-commit/scripts/export/"
tag=model-$NAME-$(sha256sum "$work/model/$NAME/MANIFEST.sha256" | cut -c1-16)
mkdir -p "$work/releases/download/$tag"
tar -C "$work/model" -czf "$work/releases/download/$tag/$NAME.tar.gz" "$NAME"
(cd "$work/releases/download/$tag" && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")

# One AMD GPU.
dev=$work/sys/bus/pci/devices/0000:03:00.0
mkdir -p "$dev"
echo 0x030000 >"$dev/class"
echo 0x1002 >"$dev/vendor"

SMART_COMMIT_SYSFS=$work/sys SMART_COMMIT_RELEASE_BASE=file://$work/releases XDG_DATA_HOME=$work/data \
    bash "$prefix/share/smart-commit/scripts/setup.sh" --no-drivers </dev/null >"$work/out" 2>&1

failures=0
expect() {
    if grep -qF -- "$1" "$work/out"; then
        echo "ok    $1"
    else
        echo "FAIL  expected: $1"
        failures=$((failures + 1))
    fi
}
expect "GPUs: 1"
expect "installed $work/data/smart-commit/models/$NAME"
# The benchmark runs unless the render group was just added; both are fine.
if grep -q "Benchmark postponed" "$work/out"; then
    expect "smart-commit bench"
else
    expect "stub smart-commit bench"
fi
expect "smart-commit hook install"
if [ "$failures" -gt 0 ]; then
    sed 's/^/        /' "$work/out"
    exit 1
fi

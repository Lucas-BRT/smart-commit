#!/usr/bin/env bash
# Runs scripts/export-model.sh with a stub uv whose Python environment has
# stub hf, optimum-cli and python commands: the flow, the manifest check,
# --check, --keep and the cleanup, and that a uv download with another
# checksum is refused. No network, Python or model.
#
#   tests/scripts/test-export-model.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
NAME=qwen2.5-coder-1.5b-instruct-int4-sym-g128
failures=0

mkdir -p "$work/scripts/export" "$work/bin"
cp "$ROOT/scripts/export-model.sh" "$work/scripts/"
cp "$ROOT/scripts/export/requirements.lock" "$ROOT/scripts/export/normalize_ir.py" "$work/scripts/export/"
: >"$work/scripts/export/MANIFEST.sha256"

# The stub uv: `venv` creates an environment of stubs, `pip sync` does
# nothing. Every command logs its arguments.
cat >"$work/bin/uv" <<'STUB'
#!/bin/sh
echo "uv $*" >>"$LOG"
[ "$1" = venv ] || exit 0
for last; do :; done
mkdir -p "$last/bin"
cat >"$last/bin/python" <<'PY'
#!/bin/sh
echo "python $*" >>"$LOG"
[ "$1" = -c ] && echo 3.11.17
exit 0
PY
cat >"$last/bin/hf" <<'HF'
#!/bin/sh
echo "hf $*" >>"$LOG"
for last; do :; done
mkdir -p "$last"
echo "Apache License 2.0" >"$last/LICENSE"
HF
cat >"$last/bin/optimum-cli" <<'OPT'
#!/bin/sh
echo "optimum-cli $*" >>"$LOG"
for last; do :; done
mkdir -p "$last"
echo "<net/>" >"$last/openvino_model.xml"
echo "weights" >"$last/openvino_model.bin"
OPT
chmod +x "$last/bin/python" "$last/bin/hf" "$last/bin/optimum-cli"
STUB
chmod +x "$work/bin/uv"

data=$work/data
cache=$work/cache
out=$data/smart-commit/models/$NAME
export LOG=$work/log
run() {
    : >"$LOG"
    XDG_DATA_HOME=$data XDG_CACHE_HOME=$cache SMART_COMMIT_UV=$work/bin/uv \
        bash "$work/scripts/export-model.sh" "$@" >"$work/out" 2>&1
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
fails() { ! "$@"; }
logged() { grep -qF -- "$1" "$LOG"; }
said() { grep -qF -- "$1" "$work/out"; }
exported() { [ -f "$out/openvino_model.xml" ] && [ -f "$out/LICENSE" ] && [ -f "$out/MANIFEST.sha256" ]; }
cleaned() { [ ! -e "$cache/smart-commit/export-venv" ] && [ ! -e "$cache/smart-commit/sources" ]; }

# A result other than the committed manifest is refused and left aside.
check "an unexpected export fails" fails run
check "and says so" said "error: the export differs from"
check "and is left for diagnosis" test -f "$out.partial/MANIFEST.sha256"
check "nothing is installed" test ! -e "$out"
check "the pinned revision is downloaded" logged "--revision 2e1fd397ee46e1388853d2af2c993145b0f1098a"
check "the NPU's flags are used" logged "--weight-format int4 --sym --group-size 128 --ratio 1.0"
check "the IR is normalized" logged "normalize_ir.py"

cp "$out.partial/MANIFEST.sha256" "$work/scripts/export/MANIFEST.sha256"
check "--check fails before the export" fails run --check
check "the expected export installs" run
check "with its files and licence" exported
check "the environment and the source are removed" cleaned
check "the licence is outside the manifest" fails grep -q LICENSE "$out/MANIFEST.sha256"

check "--check passes once exported" run --check
check "--check prints nothing" test ! -s "$work/out"
check "exporting again does nothing" run
check "and says it was there" said "already exported in $out"
check "without downloading" fails logged "hf download"

echo tampered >"$out/openvino_model.bin"
check "--check fails on a changed file" fails run --check
check "--keep exports again" run --keep
check "and keeps the environment and the source" test -d "$cache/smart-commit/export-venv" -a -d "$cache/smart-commit/sources"

# A uv download with another checksum is refused.
mkdir -p "$work/fake-uv/uv-x86_64-unknown-linux-gnu"
cp "$work/bin/uv" "$work/fake-uv/uv-x86_64-unknown-linux-gnu/uv"
tar -C "$work/fake-uv" -czf "$work/uv.tar.gz" uv-x86_64-unknown-linux-gnu
rm -rf "$out"
: >"$LOG"
if XDG_DATA_HOME=$data XDG_CACHE_HOME=$cache SMART_COMMIT_UV_URL=file://$work/uv.tar.gz \
    bash "$work/scripts/export-model.sh" >"$work/out" 2>&1; then
    uv_status=0
else
    uv_status=$?
fi
check "a uv with another checksum is refused" test "$uv_status" -ne 0
check "and named" said "is not the pinned uv 0.13.1"
check "and not kept" test ! -e "$cache/smart-commit/uv-0.13.1/uv"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all cases passed"

#!/usr/bin/env bash
# Exports the default model, downloaded from its publisher on Hugging Face,
# in the format the OpenVINO devices need (INT4 symmetric weights, which the
# NPU requires), on this machine: smart-commit does not host models. It is
# reproducible: the source model is pinned to a revision, uv and Python to
# exact versions, and every Python package to a hash (or, for the two git
# dependencies, a commit) in scripts/export/requirements.lock. The result
# carries a MANIFEST.sha256 of its files, which identifies the model in
# traces.
#
#   scripts/export-model.sh [--check] [--keep] [OUTPUT_DIR]
#
#   --check  exit 0 if the model is already exported and intact, 1 if not
#   --keep   keep the Python environment and the downloaded source model,
#            to export again faster
#
# Needs about 10 GB of disk while it runs (the source model, the Python
# environment and the export) and leaves the 0.9 GB model. Only this step
# uses Python; smart-commit itself does not. uv is downloaded, pinned, if
# this version is not in the cache. SMART_COMMIT_UV names a uv binary to use
# instead and SMART_COMMIT_UV_URL replaces its download URL, for tests.
#
# Exporting twice gives identical files; scripts/export/MANIFEST.sha256 holds
# the expected result, and the script fails if the export differs from it.
# Any change to the pinned inputs below or to the lock must update it.
#
# To change the Python packages, edit scripts/export/requirements.in and run:
#
#   uv pip compile scripts/export/requirements.in --python-version 3.11 \
#     --python-platform x86_64-manylinux_2_28 --torch-backend cpu \
#     --generate-hashes -o scripts/export/requirements.lock
set -euo pipefail

MODEL_ID=Qwen/Qwen2.5-Coder-1.5B-Instruct
MODEL_REVISION=2e1fd397ee46e1388853d2af2c993145b0f1098a
NAME=qwen2.5-coder-1.5b-instruct-int4-sym-g128
PYTHON_VERSION=3.11.17
UV_VERSION=0.13.1
UV_URL=${SMART_COMMIT_UV_URL:-https://github.com/astral-sh/uv/releases/download/$UV_VERSION/uv-x86_64-unknown-linux-gnu.tar.gz}
UV_SHA256=eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220
# The NPU needs symmetric INT4 weights for every layer (ratio 1.0); a group
# size of 128 keeps the precision of models up to about 4B parameters.
EXPORT_FLAGS=(--weight-format int4 --sym --group-size 128 --ratio 1.0)

HERE=$(cd "$(dirname "$0")" && pwd)
LOCK=$HERE/export/requirements.lock
# What the export produced when the pinned inputs were committed; any other
# result means it is not reproducible and must not be used silently.
EXPECTED=$HERE/export/MANIFEST.sha256
CHECK=0
KEEP=0
OUT=
for arg in "$@"; do
    case $arg in
        --check) CHECK=1 ;;
        --keep) KEEP=1 ;;
        -h | --help)
            sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
            exit 0
            ;;
        -*)
            echo "unknown option: $arg" >&2
            exit 2
            ;;
        *) OUT=$arg ;;
    esac
done
OUT=$(realpath -m "${OUT:-${XDG_DATA_HOME:-$HOME/.local/share}/smart-commit/models/$NAME}")
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/smart-commit
VENV=${SMART_COMMIT_EXPORT_VENV:-$CACHE/export-venv}
SOURCE=$CACHE/sources/$MODEL_ID@$MODEL_REVISION
UV=${SMART_COMMIT_UV:-$CACHE/uv-$UV_VERSION/uv}

# intact DIR: the export in DIR is the expected one, every file checked.
intact() {
    diff -q "$EXPECTED" "$1/MANIFEST.sha256" >/dev/null 2>&1 &&
        (cd "$1" && sha256sum --check --quiet MANIFEST.sha256) >/dev/null 2>&1
}

if intact "$OUT"; then
    [ "$CHECK" = 1 ] || echo "==> already exported in $OUT, every file checked"
    exit 0
fi
[ "$CHECK" = 1 ] && exit 1

if [ ! -x "$UV" ]; then
    echo "==> downloading uv $UV_VERSION"
    mkdir -p "$(dirname "$UV")"
    curl -fsSL --retry 3 -o "$UV.tar.gz" "$UV_URL"
    if ! echo "$UV_SHA256  $UV.tar.gz" | sha256sum --check --quiet - 2>/dev/null; then
        rm -f "$UV.tar.gz"
        echo "error: $UV_URL is not the pinned uv $UV_VERSION (SHA-256 differs)" >&2
        exit 1
    fi
    tar -xzf "$UV.tar.gz" -C "$(dirname "$UV")" --strip-components=1 uv-x86_64-unknown-linux-gnu/uv
    rm -f "$UV.tar.gz"
fi

echo "==> preparing Python $PYTHON_VERSION in $VENV"
if [ ! -x "$VENV/bin/python" ] || [ "$("$VENV/bin/python" -c 'import platform; print(platform.python_version())')" != "$PYTHON_VERSION" ]; then
    rm -rf "${VENV:?}"
    "$UV" venv --quiet --python "$PYTHON_VERSION" "$VENV"
fi
# uv checks every hash in the lock; the two git dependencies (optimum and
# optimum-onnx) have none, but are pinned to full commit SHAs.
"$UV" pip sync --quiet --python "$VENV/bin/python" --verify-hashes --torch-backend cpu "$LOCK"

echo "==> downloading $MODEL_ID@$MODEL_REVISION"
# optimum-cli cannot pin a revision, so the pinned one is fetched first.
"$VENV/bin/hf" download "$MODEL_ID" --revision "$MODEL_REVISION" --local-dir "$SOURCE" >/dev/null

echo "==> exporting"
staging="$OUT.partial"
rm -rf "${staging:?}"
# From inside the sources directory with a relative --model: optimum records
# the path it was given in openvino_config.json, and an absolute one would
# make the export differ between machines.
(cd "$(dirname "$SOURCE")" && "$VENV/bin/optimum-cli" export openvino \
    --model "./$(basename "$SOURCE")" \
    --task text-generation-with-past \
    "${EXPORT_FLAGS[@]}" \
    "$staging")

# OpenVINO numbers the nodes it names itself with a process-wide counter,
# which differs between machines; renumbering them makes the IR reproducible.
"$VENV/bin/python" -I "$HERE/export/normalize_ir.py" "$staging"/openvino_*.xml

lock_sha256=$(sha256sum "$LOCK" | cut -d' ' -f1)
cat >"$staging/smart-commit-export.json" <<JSON
{
  "model": "$MODEL_ID",
  "revision": "$MODEL_REVISION",
  "export_flags": "${EXPORT_FLAGS[*]}",
  "python": "$PYTHON_VERSION",
  "requirements_lock_sha256": "$lock_sha256"
}
JSON
(cd "$staging" && find . -type f ! -name MANIFEST.sha256 -printf '%P\n' | LC_ALL=C sort | xargs -d '\n' sha256sum) \
    >"$staging/MANIFEST.sha256"

if ! diff -u "$EXPECTED" "$staging/MANIFEST.sha256"; then
    echo "error: the export differs from $EXPECTED (left in $staging)." >&2
    echo "If the pinned inputs changed on purpose, commit the new manifest with them." >&2
    exit 1
fi

rm -rf "${OUT:?}"
mv "$staging" "$OUT"
# The model's licence, outside the manifest.
[ -f "$SOURCE/LICENSE" ] && cp "$SOURCE/LICENSE" "$OUT/LICENSE"
if [ "$KEEP" = 0 ]; then
    rm -rf "${SOURCE:?}"
    rmdir "$(dirname "$SOURCE")" "$CACHE/sources" 2>/dev/null || true
    # A Python environment given by SMART_COMMIT_EXPORT_VENV is not ours.
    [ -n "${SMART_COMMIT_EXPORT_VENV:-}" ] || rm -rf "${VENV:?}"
fi
echo
echo "Exported to $OUT"
echo "Model id: $(sha256sum "$OUT/MANIFEST.sha256" | cut -c1-16)"

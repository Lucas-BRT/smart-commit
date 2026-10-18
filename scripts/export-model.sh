#!/usr/bin/env bash
# Exports the default model in the format the NPU requires (INT4 symmetric
# weights), reproducibly: the source model is pinned to a revision, Python to
# an exact version, and every Python package to a hash (or, for the two git
# dependencies, a commit) in scripts/export/requirements.lock. The result carries a MANIFEST.sha256 of
# its files, which identifies the model in traces.
#
#   scripts/export-model.sh [OUTPUT_DIR]
#
# Needs uv (https://docs.astral.sh/uv/) and about 10 GB of disk for the
# download, the Python environment and the export. Only this step uses
# Python; npu-commit itself does not.
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
# The NPU needs symmetric INT4 weights for every layer (ratio 1.0); a group
# size of 128 keeps the precision of models up to about 4B parameters.
EXPORT_FLAGS=(--weight-format int4 --sym --group-size 128 --ratio 1.0)

ROOT=$(cd "$(dirname "$0")/.." && pwd)
LOCK=$ROOT/scripts/export/requirements.lock
# What the export produced when the pinned inputs were committed; any other
# result means it is not reproducible and must not be used silently.
EXPECTED=$ROOT/scripts/export/MANIFEST.sha256
OUT=$(realpath -m "${1:-${XDG_DATA_HOME:-$HOME/.local/share}/npu-commit/models/$NAME}")
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/npu-commit
VENV=${NPU_COMMIT_EXPORT_VENV:-$CACHE/export-venv}
SOURCE=$CACHE/sources/$MODEL_ID@$MODEL_REVISION

if ! command -v uv >/dev/null; then
    echo "error: uv is required: https://docs.astral.sh/uv/getting-started/installation/" >&2
    exit 1
fi

echo "==> preparing Python $PYTHON_VERSION in $VENV"
if [ ! -x "$VENV/bin/python" ] || [ "$("$VENV/bin/python" -c 'import platform; print(platform.python_version())')" != "$PYTHON_VERSION" ]; then
    rm -rf "${VENV:?}"
    uv venv --quiet --python "$PYTHON_VERSION" "$VENV"
fi
# uv checks every hash in the lock; the two git dependencies (optimum and
# optimum-onnx) have none, but are pinned to full commit SHAs.
uv pip sync --quiet --python "$VENV/bin/python" --verify-hashes --torch-backend cpu "$LOCK"

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
"$VENV/bin/python" -I "$ROOT/scripts/export/normalize_ir.py" "$staging"/openvino_*.xml

lock_sha256=$(sha256sum "$LOCK" | cut -d' ' -f1)
cat >"$staging/npu-commit-export.json" <<JSON
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
echo
echo "Exported to $OUT"
echo "Model id: $(sha256sum "$OUT/MANIFEST.sha256" | cut -c1-16)"

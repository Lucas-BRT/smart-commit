#!/usr/bin/env bash
# Downloads the OpenVINO model (for the NPU, Intel GPU and CPU) that CI
# exported with scripts/export-model.sh (.github/workflows/model.yml), and
# checks every file against scripts/export/MANIFEST.sha256: the result is
# byte for byte what an export on this machine would give, without needing
# Python, uv or 10 GB of disk.
#
#   scripts/fetch-model.sh [--dry-run]
#
# If the download is not possible, export locally with scripts/export-model.sh.
# SMART_COMMIT_RELEASE_BASE overrides where releases are downloaded from.
set -euo pipefail

NAME=qwen2.5-coder-1.5b-instruct-int4-sym-g128
HERE=$(cd "$(dirname "$0")" && pwd)
EXPECTED=$HERE/export/MANIFEST.sha256
BASE=${SMART_COMMIT_RELEASE_BASE:-https://github.com/Lucas-BRT/smart-commit/releases}
MODELS=${XDG_DATA_HOME:-$HOME/.local/share}/smart-commit/models
DRY_RUN=0
[ "${1:-}" = --dry-run ] && DRY_RUN=1

# The release is named after the manifest, so a different export can never
# be mistaken for this one.
TAG=model-$NAME-$(sha256sum "$EXPECTED" | cut -c1-16)
URL=$BASE/download/$TAG/$NAME.tar.gz

verify() {
    diff -q "$EXPECTED" "$1/MANIFEST.sha256" >/dev/null &&
        (cd "$1" && sha256sum --check --quiet MANIFEST.sha256)
}

if [ -d "$MODELS/$NAME" ] && verify "$MODELS/$NAME" 2>/dev/null; then
    echo "==> the model is already in $MODELS/$NAME and matches the manifest"
    exit 0
fi
if [ "$DRY_RUN" = 1 ]; then
    echo "would download: $URL"
    echo "would install: $MODELS/$NAME"
    exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "==> downloading $URL"
curl -fL --retry 3 -o "$work/$NAME.tar.gz" "$URL"
curl -fsL --retry 3 -o "$work/$NAME.tar.gz.sha256" "$URL.sha256"
(cd "$work" && sha256sum --check --quiet "$NAME.tar.gz.sha256")
tar -xzf "$work/$NAME.tar.gz" -C "$work"
if ! verify "$work/$NAME"; then
    echo "error: the downloaded model does not match $EXPECTED" >&2
    exit 1
fi
mkdir -p "$MODELS"
rm -rf "${MODELS:?}/$NAME"
mv "$work/$NAME" "$MODELS/$NAME"
[ -f "$work/LICENSE.Qwen2.5-Coder" ] && cp "$work/LICENSE.Qwen2.5-Coder" "$MODELS/$NAME/"
echo "==> installed $MODELS/$NAME (every file checked against the manifest)"

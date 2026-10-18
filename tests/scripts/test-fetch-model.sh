#!/usr/bin/env bash
# Runs scripts/fetch-model.sh against a fake release on disk (file:// URLs):
# a good model installs, a tampered one is refused. No network.
#
#   tests/scripts/test-fetch-model.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
NAME=qwen2.5-coder-1.5b-instruct-int4-sym-g128

# A copy of the scripts with a small fake model and its manifest.
mkdir -p "$work/scripts/export" "$work/model/$NAME"
cp "$ROOT/scripts/fetch-model.sh" "$work/scripts/"
echo "weights" >"$work/model/$NAME/openvino_model.bin"
echo "<net/>" >"$work/model/$NAME/openvino_model.xml"
(cd "$work/model/$NAME" && sha256sum openvino_model.bin openvino_model.xml >MANIFEST.sha256)
cp "$work/model/$NAME/MANIFEST.sha256" "$work/scripts/export/MANIFEST.sha256"
tag=model-$NAME-$(sha256sum "$work/scripts/export/MANIFEST.sha256" | cut -c1-16)

publish() {
    mkdir -p "$work/releases/download/$tag"
    tar -C "$work/model" -czf "$work/releases/download/$tag/$NAME.tar.gz" "$NAME"
    (cd "$work/releases/download/$tag" && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")
}

fetch() {
    SMART_COMMIT_RELEASE_BASE=file://$work/releases XDG_DATA_HOME=$work/data \
        bash "$work/scripts/fetch-model.sh" >"$work/out" 2>&1
}

publish
fetch
grep -q "every file checked against the manifest" "$work/out"
test -f "$work/data/smart-commit/models/$NAME/openvino_model.bin"
echo "ok    a published model installs"

fetch
grep -q "already in" "$work/out"
echo "ok    an installed model is not downloaded again"

rm -rf "$work/data"
echo "tampered" >"$work/model/$NAME/openvino_model.bin"
publish
if fetch; then
    echo "FAIL  a tampered model was installed"
    exit 1
fi
grep -q "does not match" "$work/out"
test ! -e "$work/data/smart-commit/models/$NAME"
echo "ok    a model that does not match the manifest is refused"

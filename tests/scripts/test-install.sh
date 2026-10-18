#!/usr/bin/env bash
# Packages twice (the tarball must be byte-identical), then installs from a
# fake release on disk (file:// URLs), refuses a tampered download and
# uninstalls. No network or sudo.
#
#   tests/scripts/test-install.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=npu-commit-x86_64-unknown-linux-gnu.tar.gz
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -1)

"$ROOT/scripts/package.sh" "$work/dist1" >/dev/null
"$ROOT/scripts/package.sh" "$work/dist2" >/dev/null
cmp "$work/dist1/SHA256SUMS" "$work/dist2/SHA256SUMS"
echo "ok    packaging twice gives the same tarball: $(cut -c1-16 "$work/dist1/SHA256SUMS")"

# GitHub's layout: latest/download/ and download/v<version>/.
for dir in latest/download "download/v$VERSION"; do
    mkdir -p "$work/releases/$dir"
    cp "$work/dist1/$ASSET" "$work/dist1/SHA256SUMS" "$work/releases/$dir/"
done

install() {
    NPU_COMMIT_RELEASE_BASE=file://$work/releases bash "$ROOT/install.sh" --prefix "$work/prefix" "$@" \
        >"$work/out" 2>&1
}

install --no-setup
"$work/prefix/bin/npu-commit" --version | grep -qx "npu-commit $VERSION"
for file in share/npu-commit/scripts/setup.sh share/npu-commit/scripts/setup-gpu.sh \
    share/npu-commit/scripts/export/MANIFEST.sha256 share/doc/npu-commit/LICENSE share/doc/npu-commit/uso.md; do
    test -f "$work/prefix/$file"
done
grep -q "npu-commit setup" "$work/out"
echo "ok    installs the latest release"

install --no-setup --version "v$VERSION"
grep -q "installed npu-commit $VERSION" "$work/out"
echo "ok    installs a given version"

printf 'x' >>"$work/releases/latest/download/$ASSET"
if install --no-setup; then
    echo "FAIL  a tampered download was installed"
    exit 1
fi
grep -q "does not match the release's SHA256SUMS" "$work/out"
echo "ok    refuses a download that does not match SHA256SUMS"

install --uninstall
test ! -e "$work/prefix/bin/npu-commit"
test ! -e "$work/prefix/share/npu-commit"
echo "ok    uninstalls"

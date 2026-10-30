#!/usr/bin/env bash
# Packages twice (the tarball must be byte-identical), then installs from a
# fake release on disk (file:// URLs), refuses a tampered download and
# uninstalls. No network or sudo.
#
#   cli/tests/scripts/test-install.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/cli/Cargo.toml" | head -1)

"$ROOT/scripts/package.sh" "$work/dist1" >/dev/null
"$ROOT/scripts/package.sh" "$work/dist2" >/dev/null
cmp "$work/dist1/SHA256SUMS" "$work/dist2/SHA256SUMS"
echo "ok    packaging twice gives the same tarball: $(cut -c1-16 "$work/dist1/SHA256SUMS")"

# The installer is a release asset of its own, and SHA256SUMS lists it.
cmp "$ROOT/install.sh" "$work/dist1/install.sh"
grep -q " install.sh\$" "$work/dist1/SHA256SUMS"
(cd "$work/dist1" && sha256sum --check --quiet SHA256SUMS)
echo "ok    install.sh is published beside the tarball, covered by SHA256SUMS"

# GitHub's layout: latest/download/ and download/v<version>/.
for dir in latest/download "download/v$VERSION"; do
    mkdir -p "$work/releases/$dir"
    cp "$work/dist1/$ASSET" "$work/dist1/install.sh" "$work/dist1/SHA256SUMS" "$work/releases/$dir/"
done

install() {
    SMART_COMMIT_RELEASE_BASE=file://$work/releases bash "$ROOT/install.sh" --prefix "$work/prefix" "$@" \
        >"$work/out" 2>&1
}

install --no-setup
"$work/prefix/bin/smart-commit" --version | grep -qx "smart-commit $VERSION"
for file in share/smart-commit/scripts/setup-gpu.sh share/smart-commit/scripts/setup-npu.sh \
    share/smart-commit/scripts/check-npu.sh share/smart-commit/scripts/setup-llamacpp.sh \
    share/smart-commit/scripts/export-model.sh share/smart-commit/scripts/install.sh \
    share/smart-commit/scripts/export/MANIFEST.sha256 \
    share/smart-commit/scripts/export/requirements.lock share/smart-commit/scripts/export/normalize_ir.py \
    share/doc/smart-commit/LICENSE share/doc/smart-commit/usage.md; do
    test -f "$work/prefix/$file"
done
test ! -e "$work/prefix/share/smart-commit/scripts/fetch-model.sh"
grep -q "smart-commit setup" "$work/out"
echo "ok    installs the latest release"
# install.sh only replaces and removes these, since share/smart-commit is
# also the data directory.
release_files=$(tar -tzf "$work/dist1/$ASSET" | sed -n 's|^[^/]*/share/smart-commit/\([^/][^/]*\).*|\1|p' | LC_ALL=C sort -u | tr '\n' ' ')
[ "$release_files" = "VERSION scripts " ] || {
    echo "FAIL  share/smart-commit has $release_files; update RELEASE_FILES in install.sh"
    exit 1
}
echo "ok    the release's share/smart-commit is what install.sh manages"

install --no-setup --version "v$VERSION"
grep -q "installed smart-commit $VERSION" "$work/out"
echo "ok    installs a given version"

printf 'x' >>"$work/releases/latest/download/$ASSET"
if install --no-setup; then
    echo "FAIL  a tampered download was installed"
    exit 1
fi
grep -q "does not match the release's SHA256SUMS" "$work/out"
echo "ok    refuses a download that does not match SHA256SUMS"

install --uninstall
test ! -e "$work/prefix/bin/smart-commit"
test ! -e "$work/prefix/share/smart-commit"
echo "ok    uninstalls"

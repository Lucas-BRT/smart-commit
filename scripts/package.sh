#!/usr/bin/env bash
# Builds the release tarball, reproducibly: the same commit gives the same
# bytes on any machine (pinned toolchain, --locked, no build paths in the
# binary, and a tar with fixed order, owner and dates from the commit).
#
#   scripts/package.sh [OUT_DIR]     (default: dist/)
#
# Writes OUT_DIR/npu-commit-x86_64-unknown-linux-gnu.tar.gz and
# OUT_DIR/SHA256SUMS.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/dist}
TARGET=x86_64-unknown-linux-gnu
NAME=npu-commit-$TARGET
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
export SOURCE_DATE_EPOCH

# Paths of this machine must not end up in the binary.
export RUSTFLAGS="--remap-path-prefix=$ROOT=. --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
cargo build --release --locked --target "$TARGET" --quiet

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage=$work/$NAME
mkdir -p "$stage/bin" "$stage/share/npu-commit/scripts/export" "$stage/share/doc/npu-commit"
install -m 755 "target/$TARGET/release/npu-commit" "$stage/bin/"
for script in setup.sh setup-npu.sh setup-gpu.sh check-npu.sh fetch-model.sh export-model.sh; do
    install -m 755 "scripts/$script" "$stage/share/npu-commit/scripts/"
done
install -m 644 scripts/export/requirements.in scripts/export/requirements.lock \
    scripts/export/MANIFEST.sha256 "$stage/share/npu-commit/scripts/export/"
install -m 644 LICENSE README.md docs/uso.md "$stage/share/doc/npu-commit/"
echo "$VERSION" >"$stage/share/npu-commit/VERSION"

mkdir -p "$OUT"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
    -C "$work" -cf - "$NAME" | gzip -9n >"$OUT/$NAME.tar.gz"
(cd "$OUT" && sha256sum "$NAME.tar.gz" >SHA256SUMS)
echo "npu-commit $VERSION: $OUT/$NAME.tar.gz"
cat "$OUT/SHA256SUMS"

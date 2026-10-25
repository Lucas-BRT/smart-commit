#!/usr/bin/env bash
# Builds the release tarball, reproducibly: the same commit gives the same
# bytes on any machine. The binary is built in the pinned container of
# scripts/build/Dockerfile (the host's C toolchain would otherwise end up in
# it), with fixed paths remapped out of it; the tarball has fixed order,
# owner, modes and dates (the commit's).
#
#   scripts/package.sh [OUT_DIR]     (default: dist/)
#
# Needs Docker. SMART_COMMIT_PACKAGE_HOST=1 builds on the host instead: the
# result is then only reproducible on machines with the same C toolchain.
#
# Writes OUT_DIR/smart-commit-x86_64-unknown-linux-gnu.tar.gz and
# OUT_DIR/SHA256SUMS.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(realpath -m "${1:-$ROOT/dist}")
TARGET=x86_64-unknown-linux-gnu
NAME=smart-commit-$TARGET
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
export SOURCE_DATE_EPOCH

binary=$(mktemp -d)
if [ "${SMART_COMMIT_PACKAGE_HOST:-0}" = 1 ]; then
    RUSTFLAGS="--remap-path-prefix=$ROOT=. --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo" \
        cargo build --release --locked --target "$TARGET" --quiet
    cp "target/$TARGET/release/smart-commit" "$binary/"
else
    docker build --quiet --tag smart-commit-build scripts/build >/dev/null
    # The sources are mounted read-only at /src; the toolchain, the crates
    # and the build go in volumes, reused between runs.
    docker run --rm \
        --volume "$ROOT:/src:ro" \
        --volume "$binary:/out" \
        --volume smart-commit-rustup:/root/.rustup \
        --volume smart-commit-cargo:/root/.cargo/registry \
        --volume smart-commit-target:/target \
        --workdir /src \
        --env CARGO_TARGET_DIR=/target \
        --env RUSTFLAGS="--remap-path-prefix=/src=. --remap-path-prefix=/root/.cargo=/cargo" \
        --env HOST_UID="$(id -u)" --env HOST_GID="$(id -g)" \
        smart-commit-build sh -c "cargo build --release --locked --target $TARGET --quiet \
            && cp /target/$TARGET/release/smart-commit /out/ \
            && chown \$HOST_UID:\$HOST_GID /out/smart-commit"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage=$work/$NAME
mkdir -p "$stage/bin" "$stage/share/smart-commit/scripts/export" "$stage/share/doc/smart-commit"
install -m 755 "$binary/smart-commit" "$stage/bin/"
rm -rf "$binary"
for script in setup-npu.sh setup-gpu.sh setup-llamacpp.sh check-npu.sh export-model.sh; do
    install -m 755 "scripts/$script" "$stage/share/smart-commit/scripts/"
done
# `smart-commit update` and `uninstall` run the installer they came with.
install -m 755 install.sh "$stage/share/smart-commit/scripts/"
install -m 644 scripts/export/requirements.in scripts/export/requirements.lock \
    scripts/export/MANIFEST.sha256 scripts/export/normalize_ir.py "$stage/share/smart-commit/scripts/export/"
install -m 644 LICENSE README.md docs/usage.md "$stage/share/doc/smart-commit/"
echo "$VERSION" >"$stage/share/smart-commit/VERSION"

mkdir -p "$OUT"
# Modes are normalised too: the umask of whoever packages must not matter.
# The same script on the host and in the image: pack DIR OUT NAME.
# shellcheck disable=SC2016
PACK='tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --mode=u+rwX,go+rX,go-w \
    --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
    -C "$1" -cf - "$3" | gzip -9n >"$2/$3.tar.gz"'
if [ "${SMART_COMMIT_PACKAGE_HOST:-0}" = 1 ]; then
    sh -c "$PACK" pack "$work" "$OUT" "$NAME"
else
    # tar's and gzip's versions change the bytes too, so they come from the
    # image as well.
    docker run --rm \
        --volume "$work:/work:ro" \
        --volume "$OUT:/out" \
        --env SOURCE_DATE_EPOCH \
        --env HOST_UID="$(id -u)" --env HOST_GID="$(id -g)" \
        smart-commit-build sh -c "$PACK && chown \$HOST_UID:\$HOST_GID /out/\$3.tar.gz" \
        pack /work /out "$NAME"
fi
(cd "$OUT" && sha256sum "$NAME.tar.gz" >SHA256SUMS)
echo "smart-commit $VERSION: $OUT/$NAME.tar.gz"
cat "$OUT/SHA256SUMS"

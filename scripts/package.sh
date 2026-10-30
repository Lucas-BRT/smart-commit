#!/usr/bin/env bash
# Builds the release tarball, reproducibly: the same commit gives the same
# bytes on any machine. The binary is built in the pinned container of
# scripts/build/Dockerfile (the host's C toolchain would otherwise end up in
# it), with fixed paths remapped out of it; the tarball has fixed order,
# owner, modes and dates (the commit's).
#
#   scripts/package.sh [OUT_DIR]     (default: dist/)
#   scripts/package.sh --check-env   (does this machine build the same bytes?)
#
# Needs Docker. The image is the one in scripts/build/IMAGE (pulled, by
# digest), or SMART_COMMIT_BUILD_IMAGE, or built from scripts/build/Dockerfile
# when neither is set. SMART_COMMIT_PACKAGE_HOST=1 builds on the host instead:
# the result is then only reproducible on a machine whose C toolchain
# `--check-env` finds the same as the image's.
#
# Writes OUT_DIR/smart-commit-x86_64-unknown-linux-gnu.tar.gz, a copy of
# install.sh (the installer of this very commit, a release asset of its own),
# OUT_DIR/BUILDINFO (what it was built with) and OUT_DIR/SHA256SUMS, which
# lists all three.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
if [ "${1:-}" = --check-env ]; then
    exec sh "$ROOT/scripts/build/check-env.sh"
fi
OUT=$(realpath -m "${1:-$ROOT/dist}")
TARGET=x86_64-unknown-linux-gnu
NAME=smart-commit-$TARGET
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' cli/Cargo.toml | head -1)
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
export SOURCE_DATE_EPOCH

binary=$(mktemp -d)
if [ "${SMART_COMMIT_PACKAGE_HOST:-0}" = 1 ]; then
    RUSTFLAGS="--remap-path-prefix=$ROOT=. --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo" \
        cargo build --release --locked --target "$TARGET" --quiet
    cp "target/$TARGET/release/smart-commit" "$binary/"
else
    # shellcheck source=scripts/build/image.sh
    . "$ROOT/scripts/build/image.sh"
    resolve_image
    # The sources are mounted read-only at /src; the toolchain, the crates
    # and the build go in volumes, reused between runs. With
    # SMART_COMMIT_BUILD_CACHE=DIR the toolchain and the crates are in DIR
    # instead, which CI keeps between runs. The build (the target volume) is
    # not: CI starts every run with a new one, so the tarball's hash is that of
    # a build from scratch.
    rustup=smart-commit-rustup cargo=smart-commit-cargo
    if [ -n "${SMART_COMMIT_BUILD_CACHE:-}" ]; then
        mkdir -p "$SMART_COMMIT_BUILD_CACHE/rustup" "$SMART_COMMIT_BUILD_CACHE/cargo"
        rustup=$(realpath "$SMART_COMMIT_BUILD_CACHE/rustup")
        cargo=$(realpath "$SMART_COMMIT_BUILD_CACHE/cargo")
    fi
    build_volumes=(
        --volume "$ROOT:/src:ro"
        --volume "$rustup:/root/.rustup"
        --volume "$cargo:/root/.cargo/registry"
        --volume smart-commit-target:/target
        --workdir /src
        --env CARGO_TARGET_DIR=/target
    )
    # The network is for what the build needs to get (the toolchain, from
    # rust-toolchain.toml, and the crates of Cargo.lock) and nothing else: the
    # build itself runs without it, so nothing can be fetched into the binary
    # behind the lock file's back.
    docker run --rm "${build_volumes[@]}" "$IMAGE" sh -c "cargo fetch --locked --quiet"
    docker run --rm --network none "${build_volumes[@]}" \
        --volume "$binary:/out" \
        --env RUSTFLAGS="--remap-path-prefix=/src=. --remap-path-prefix=/root/.cargo=/cargo" \
        --env HOST_UID="$(id -u)" --env HOST_GID="$(id -g)" \
        "$IMAGE" sh -c "cargo build --release --locked --offline --target $TARGET --quiet \
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
        "$IMAGE" sh -c "$PACK && chown \$HOST_UID:\$HOST_GID /out/\$3.tar.gz" \
        pack /work /out "$NAME"
fi
# The installer is published beside the tarball, not in it (it is in the
# tarball too, for `update`): the script and the binary of a release match.
install -m 755 install.sh "$OUT/install.sh"
# What it was built with, so that anyone can see whether their machine does the
# same (`scripts/package.sh --check-env`) and where a difference would come from.
{
    echo "smart-commit $VERSION, commit $(git rev-parse HEAD)"
    if [ "${SMART_COMMIT_PACKAGE_HOST:-0}" = 1 ]; then
        echo "built on the host"
        sh scripts/build/env-report.sh
    else
        echo "built in the image $IMAGE"
        docker run --rm --volume "$ROOT/scripts/build:/build:ro" "$IMAGE" sh /build/env-report.sh
    fi
    sed -n 's/^channel = "\(.*\)"$/rust \1/p' rust-toolchain.toml
} >"$OUT/BUILDINFO"
(cd "$OUT" && sha256sum "$NAME.tar.gz" install.sh BUILDINFO >SHA256SUMS)
echo "smart-commit $VERSION: $OUT/$NAME.tar.gz"
cat "$OUT/SHA256SUMS"

#!/bin/sh
# Sourced by scripts/package.sh: sets IMAGE to the image the release is built
# in, getting it first if needed.
#
#   1. SMART_COMMIT_BUILD_IMAGE, if set: that image, pulled if it is not here;
#   2. the reference in scripts/build/IMAGE, if it has one: pulled;
#   3. otherwise, built from scripts/build/Dockerfile (needs the network,
#      and snapshot.ubuntu.com for the packages).
#
# Needs ROOT (the repository) to be set.
resolve_image() {
    if [ -n "${SMART_COMMIT_BUILD_IMAGE:-}" ]; then
        IMAGE=$SMART_COMMIT_BUILD_IMAGE
    else
        IMAGE=$(sed -e '/^[[:space:]]*#/d' -e '/^[[:space:]]*$/d' "$ROOT/scripts/build/IMAGE" 2>/dev/null | head -n 1 || true)
    fi
    if [ -n "$IMAGE" ]; then
        docker image inspect "$IMAGE" >/dev/null 2>&1 || docker pull --quiet "$IMAGE" >/dev/null
    else
        IMAGE=smart-commit-build
        docker build --quiet --tag "$IMAGE" "$ROOT/scripts/build" >/dev/null
    fi
}

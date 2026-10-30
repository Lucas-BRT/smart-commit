#!/usr/bin/env bash
# scripts/build/image.sh with a fake `docker`: which image the release is built
# in, and whether it is pulled or built. No Docker, no network.
#
#   cli/tests/scripts/test-build-image.sh
set -euo pipefail

SCRIPT=$(cd "$(dirname "$0")/../../.." && pwd)/scripts/build/image.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

mkdir -p "$work/bin" "$work/root/scripts/build"
# A docker that records its commands; `image inspect` succeeds when the image
# is "local" (FAKE_LOCAL set).
cat >"$work/bin/docker" <<'DOCKER'
#!/bin/sh
echo "$*" >>"$FAKE_LOG"
case "$1 $2" in
    "image inspect") [ -n "${FAKE_LOCAL:-}" ] ;;
esac
DOCKER
chmod +x "$work/bin/docker"

DIGEST=ghcr.io/lucas-brt/smart-commit-build@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

# resolve [LOCAL]: runs resolve_image with scripts/build/IMAGE as set up by the case.
resolve() {
    : >"$work/log"
    (
        export FAKE_LOG=$work/log PATH=$work/bin:$PATH
        [ -z "${1:-}" ] || export FAKE_LOCAL=1
        ROOT=$work/root
        # shellcheck source=scripts/build/image.sh
        . "$SCRIPT"
        resolve_image
        echo "$IMAGE" >"$work/image"
    )
}
check() {
    local name=$1
    shift
    if "$@"; then
        echo "ok    $name"
    else
        echo "FAIL  $name"
        sed 's/^/        /' "$work/log"
        failures=$((failures + 1))
    fi
}
used() { [ "$(cat "$work/image")" = "$1" ]; }
ran() { grep -qF -- "$1" "$work/log"; }
not() { ! "$@"; }

printf '# comment\n\n%s\n' "$DIGEST" >"$work/root/scripts/build/IMAGE"
resolve
check "a reference in IMAGE is the image" used "$DIGEST"
check "a reference that is not here is pulled" ran "pull --quiet $DIGEST"
check "a reference is never built" not ran "build --quiet"
resolve local
check "a reference that is here is not pulled" not ran "pull"

printf '# no image published yet\n' >"$work/root/scripts/build/IMAGE"
resolve
check "no reference: the image is built from the Dockerfile" ran "build --quiet --tag smart-commit-build $work/root/scripts/build"
check "no reference: its tag is the image" used smart-commit-build
check "no reference: nothing is pulled" not ran "pull"

rm "$work/root/scripts/build/IMAGE"
resolve
check "no IMAGE file: built from the Dockerfile" used smart-commit-build

printf '%s\n' "$DIGEST" >"$work/root/scripts/build/IMAGE"
SMART_COMMIT_BUILD_IMAGE=my/image:1 resolve
check "the variable overrides IMAGE" used my/image:1
check "the variable's image is pulled when it is not here" ran "pull --quiet my/image:1"
check "the variable's image does not build" not ran "build --quiet"
SMART_COMMIT_BUILD_IMAGE=my/image:1 resolve local
check "the variable's image is used as it is when it is here" not ran "pull"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all checks passed"

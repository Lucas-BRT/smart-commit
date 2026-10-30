#!/bin/sh
# Says whether this machine has the C toolchain the release is built with, so
# a package made here has the same bytes as the release's.
#
#   scripts/build/check-env.sh        (scripts/package.sh --check-env)
#
# Compares scripts/build/env-report.sh here with scripts/build/ENV, which was
# made in the reference image. Exit status: 0 when every version the two have
# in common is the same, 1 when one differs. The versions are what decides
# the bytes, but a distribution that patches a package can change them with
# the same version: the hash in the release's SHA256SUMS is the proof.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
reference=${SMART_COMMIT_ENV_FILE:-$here/ENV}
mine=$(mktemp)
trap 'rm -f "$mine"' EXIT
sh "$here/env-report.sh" >"$mine"

differs=0
packages_compared=0
while read -r kind name want; do
    [ -n "$kind" ] || continue
    have=$(awk -v k="$kind" -v n="$name" '$1 == k && $2 == n { print $3 }' "$mine")
    if [ "$kind" = package ]; then
        # A package the machine does not have (or cannot name) is not a difference.
        [ -n "$have" ] || continue
        packages_compared=1
    fi
    if [ "$have" != "$want" ]; then
        [ "$differs" = 1 ] || echo "This machine's build environment differs from the reference image:"
        printf '  %-8s %-10s here %-30s reference %s\n' "$kind" "$name" "${have:-none}" "$want"
        differs=1
    fi
done <"$reference"

if [ "$differs" = 1 ]; then
    echo "A package built here is likely to have other bytes than the release's."
    echo "Build it in the reference image instead: scripts/package.sh (needs Docker)."
    exit 1
fi
echo "This machine's build environment matches the reference image (scripts/build/ENV)."
[ "$packages_compared" = 1 ] ||
    echo "The distribution's packages were not compared (no dpkg here): only the tool versions."
echo "A package built here should have the same bytes as the release's; compare the hash with SHA256SUMS."

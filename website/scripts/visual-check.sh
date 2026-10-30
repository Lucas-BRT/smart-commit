#!/usr/bin/env bash
# Screenshots one or more paths of the running site at three device sizes.
#   scripts/visual-check.sh [-u BASE_URL] [-o OUT_DIR] PATH...
# Needs Firefox. Each shot uses a fresh profile, so no stylesheet is cached
# and a running Firefox does not get in the way.
set -euo pipefail

base=http://127.0.0.1:3000
out=${TMPDIR:-/tmp}/visual-check
while getopts "u:o:" opt; do
  case $opt in
    u) base=$OPTARG ;;
    o) out=$OPTARG ;;
    *) exit 2 ;;
  esac
done
shift $((OPTIND - 1))
[ $# -gt 0 ] || { echo "usage: $0 [-u BASE_URL] [-o OUT_DIR] PATH..." >&2; exit 2; }

mkdir -p "$out"
for path in "$@"; do
  name=${path#/}; name=${name:-home}; name=${name//\//-}
  for device in desktop:1280 tablet:820 mobile:390; do
    profile=$(mktemp -d)
    timeout 90 firefox --no-remote --profile "$profile" --headless \
      --screenshot "$out/$name-${device%%:*}.png" \
      --window-size="${device##*:},3200" "$base$path" >/dev/null 2>&1 || echo "failed: $path ${device%%:*}" >&2
    rm -rf "$profile"
  done
done
echo "$out"

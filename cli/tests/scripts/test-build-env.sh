#!/usr/bin/env bash
# scripts/build/check-env.sh against fake tools and a reference file: it says
# whether the machine has the versions the release is built with. No Docker.
#
#   cli/tests/scripts/test-build-env.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

bin=$work/bin
mkdir -p "$bin"

# fake_tools GCC LD GLIBC TAR GZIP [DPKG_VERSION]: the tools print these versions.
fake_tools() {
    printf '#!/bin/sh\necho %s\n' "$1" >"$bin/gcc"
    printf '#!/bin/sh\necho "GNU ld (GNU Binutils) %s"\n' "$2" >"$bin/ld"
    printf '#!/bin/sh\necho "glibc %s"\n' "$3" >"$bin/getconf"
    printf '#!/bin/sh\necho "tar (GNU tar) %s"\n' "$4" >"$bin/tar"
    printf '#!/bin/sh\necho "gzip %s"\n' "$5" >"$bin/gzip"
    if [ -n "${6:-}" ]; then
        printf '#!/bin/sh\necho -n "%s"\n' "$6" >"$bin/dpkg-query"
    else
        printf '#!/bin/sh\nexit 1\n' >"$bin/dpkg-query"
    fi
    chmod +x "$bin"/*
}

cat >"$work/ENV" <<'REF'
tool gcc 11.4.0
tool ld 2.38
tool glibc 2.35
tool tar 1.34
tool gzip 1.10
package libc6-dev 2.35-0ubuntu3.15
REF

run() {
    SMART_COMMIT_ENV_FILE=$work/ENV PATH=$bin:/usr/bin:/bin sh "$ROOT/scripts/build/check-env.sh" >"$work/out" 2>&1
}
check() {
    local name=$1
    shift
    if "$@"; then
        echo "ok    $name"
    else
        echo "FAIL  $name"
        sed 's/^/        /' "$work/out"
        failures=$((failures + 1))
    fi
}
not() { ! "$@"; }
said() { grep -qF -- "$1" "$work/out"; }

fake_tools 11.4.0 2.38 2.35 1.34 1.10 2.35-0ubuntu3.15
check "the same versions: succeeds" run
check "the same versions: says it matches" said "matches the reference image"
check "the same versions: no warning about the packages" not said "were not compared"

fake_tools 13.3.0 2.38 2.35 1.34 1.10 2.35-0ubuntu3.15
check "another gcc: fails" not run
check "another gcc: names it with both versions" said "gcc        here 13.3.0"
check "another gcc: names the reference too" said "reference 11.4.0"
check "another gcc: does not blame the rest" not said "tar        here"
check "another gcc: points at the reference image" said "scripts/package.sh (needs Docker)"

fake_tools 11.4.0 2.38 2.39 1.34 1.10 2.35-0ubuntu3.15
check "another glibc: fails" not run
check "another glibc: says so" said "glibc      here 2.39"

fake_tools 11.4.0 2.38 2.35 1.34 1.10 2.35-0ubuntu3.99
check "the same tools but a patched package: fails" not run
check "a patched package: shows both builds" said "here 2.35-0ubuntu3.99"

# A machine without dpkg: only the tool versions are compared, and it says so.
fake_tools 11.4.0 2.38 2.35 1.34 1.10
check "no package names: succeeds when the tools match" run
check "no package names: says they were not compared" said "were not compared"

# A tool that is missing counts as a difference.
fake_tools 11.4.0 2.38 2.35 1.34 1.10
rm "$bin/gcc"
printf '#!/bin/sh\nexit 1\n' >"$bin/gcc"
chmod +x "$bin/gcc"
check "a missing tool: fails" not run

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all checks passed"

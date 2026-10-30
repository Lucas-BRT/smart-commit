#!/bin/sh
# Prints the versions of what ends up in the release's bytes: the C compiler
# and linker that link the binary, the glibc whose runtime objects are
# linked into it, and the tar and gzip that make the archive.
#
#   scripts/build/env-report.sh
#
# `tool` lines are portable: the same on any distribution that has these
# versions. `package` lines (Debian and Ubuntu only) are the distribution's
# own builds of them, which is what the reference image has. The reference
# is scripts/build/ENV, made by running this script in the image.
set -eu

version_of() { # "tool (GNU thing) 1.2.3" -> 1.2.3
    "$@" 2>/dev/null | sed -n '1s/.* //p'
}

echo "tool gcc $(gcc -dumpfullversion 2>/dev/null || echo missing)"
echo "tool ld $(version_of ld --version || echo missing)"
echo "tool glibc $(getconf GNU_LIBC_VERSION 2>/dev/null | sed 's/^glibc //' || echo missing)"
echo "tool tar $(version_of tar --version || echo missing)"
echo "tool gzip $(version_of gzip --version || echo missing)"

if command -v dpkg-query >/dev/null 2>&1; then
    for pkg in gcc-11 binutils libc6-dev tar gzip; do
        v=$(dpkg-query -W -f='${Version}' "$pkg" 2>/dev/null || true)
        if [ -n "$v" ]; then echo "package $pkg $v"; fi
    done
fi

#!/usr/bin/env bash
# Installs smart-commit from its GitHub releases, without sudo, then runs the
# guided setup (drivers, models and the benchmark that picks the device).
#
#   curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash
#
# Options (after `bash -s --` when piping):
#   --version X.Y.Z  install that release instead of the latest
#   --prefix DIR     install in DIR/bin and DIR/share (default: ~/.local)
#   --no-setup       only install; run `smart-commit setup` later
#   --yes            answer yes to the setup's questions
#   --uninstall      remove smart-commit (models, caches and config are kept)
#
# The download is checked against the release's SHA256SUMS.
# SMART_COMMIT_RELEASE_BASE overrides where releases are downloaded from.
set -euo pipefail

BASE=${SMART_COMMIT_RELEASE_BASE:-https://github.com/Lucas-BRT/smart-commit/releases}
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
PREFIX=$HOME/.local
VERSION=latest
SETUP=1
UNINSTALL=0
setup_args=()

while [ $# -gt 0 ]; do
    case $1 in
        --version)
            VERSION=${2:?--version needs a value}
            VERSION=${VERSION#v}
            shift
            ;;
        --prefix)
            PREFIX=${2:?--prefix needs a directory}
            shift
            ;;
        --no-setup) SETUP=0 ;;
        --yes) setup_args+=(--yes) ;;
        --uninstall) UNINSTALL=1 ;;
        -h | --help)
            sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            exit 2
            ;;
    esac
    shift
done

say() { printf '==> %s\n' "$*"; }
die() {
    echo "error: $*" >&2
    exit 1
}

if [ "$UNINSTALL" = 1 ]; then
    rm -f "$PREFIX/bin/smart-commit"
    rm -rf "${PREFIX:?}/share/smart-commit" "$PREFIX/share/doc/smart-commit"
    say "removed smart-commit from $PREFIX"
    echo "Kept (delete them yourself if you want):"
    echo "  models and benchmark: ${XDG_DATA_HOME:-$HOME/.local/share}/smart-commit"
    echo "  compiled-model cache: ${XDG_CACHE_HOME:-$HOME/.cache}/smart-commit"
    echo "  configuration:        ${XDG_CONFIG_HOME:-$HOME/.config}/smart-commit"
    echo "Repositories with the hook keep it; \`smart-commit hook uninstall\` needs the binary,"
    echo "so remove .git/hooks/prepare-commit-msg by hand there."
    exit 0
fi

[ "$(uname -s)" = Linux ] || die "smart-commit runs on Linux only"
[ "$(uname -m)" = x86_64 ] || die "smart-commit releases are built for x86_64 only"
for tool in curl tar sha256sum; do
    command -v "$tool" >/dev/null || die "$tool is required"
done

if [ "$VERSION" = latest ]; then
    url=$BASE/latest/download
else
    url=$BASE/download/v$VERSION
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
say "downloading $url/$ASSET"
curl -fL --retry 3 -o "$work/$ASSET" "$url/$ASSET"
curl -fsL --retry 3 -o "$work/SHA256SUMS" "$url/SHA256SUMS"
(cd "$work" && grep " $ASSET\$" SHA256SUMS | sha256sum --check --quiet -) ||
    die "the download does not match the release's SHA256SUMS"

tar -xzf "$work/$ASSET" -C "$work"
src=$work/smart-commit-x86_64-unknown-linux-gnu
mkdir -p "$PREFIX/bin" "$PREFIX/share/doc"
install -m 755 "$src/bin/smart-commit" "$PREFIX/bin/smart-commit"
rm -rf "${PREFIX:?}/share/smart-commit" "$PREFIX/share/doc/smart-commit"
cp -R "$src/share/smart-commit" "$PREFIX/share/smart-commit"
cp -R "$src/share/doc/smart-commit" "$PREFIX/share/doc/smart-commit"
say "installed $("$PREFIX/bin/smart-commit" --version) in $PREFIX"

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "note: $PREFIX/bin is not on your PATH; add it to your shell's profile." ;;
esac

if [ "$SETUP" = 1 ]; then
    "$PREFIX/share/smart-commit/scripts/setup.sh" "${setup_args[@]}"
else
    echo "Run \`smart-commit setup\` to set up the devices and models."
fi

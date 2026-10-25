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
#   --update         install the release only if it is not the version
#                    installed, without the setup; llama.cpp, if installed,
#                    follows the build the new release pins (no sudo).
#                    `smart-commit update` runs this
#   --yes            answer yes to the setup's questions
#   --uninstall      remove smart-commit (models, caches and config are kept)
#   --purge          --uninstall, and also the models, llama.cpp, the
#                    benchmark, the caches and the configuration
#                    (`smart-commit uninstall` runs this)
#   --system         --uninstall, and also OpenVINO, if smart-commit's setup
#                    installed it (/opt/intel, /etc/ld.so.conf.d; uses sudo,
#                    asks first unless --yes). Driver packages are kept.
#
# The download is checked against the release's SHA256SUMS.
# SMART_COMMIT_RELEASE_BASE overrides where releases are downloaded from;
# SMART_COMMIT_SYSTEM_ROOT (a directory standing for /) and
# SMART_COMMIT_SUDO (the command instead of sudo) are for tests.
set -euo pipefail

BASE=${SMART_COMMIT_RELEASE_BASE:-https://github.com/Lucas-BRT/smart-commit/releases}
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
PREFIX=$HOME/.local
VERSION=latest
SETUP=1
UPDATE=0
UNINSTALL=0
PURGE=0
SYSTEM=0
ASSUME_YES=0
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
        --update) UPDATE=1 SETUP=0 ;;
        --yes)
            setup_args+=(--yes)
            ASSUME_YES=1
            ;;
        --uninstall) UNINSTALL=1 ;;
        --purge) UNINSTALL=1 PURGE=1 ;;
        --system) UNINSTALL=1 SYSTEM=1 ;;
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

# What the release puts in share/smart-commit. With the default prefix that
# directory is also $XDG_DATA_HOME/smart-commit, where the setup keeps the
# models, llama.cpp and the benchmark: only these entries are ever replaced
# or removed.
RELEASE_FILES=(scripts VERSION)

# remove_release: the files this script installed, and nothing else.
remove_release() {
    local entry
    rm -f "$PREFIX/bin/smart-commit"
    for entry in "${RELEASE_FILES[@]}"; do
        rm -rf "${PREFIX:?}/share/smart-commit/$entry"
    done
    rmdir "$PREFIX/share/smart-commit" 2>/dev/null || true
    rm -rf "${PREFIX:?}/share/doc/smart-commit"
}
die() {
    echo "error: $*" >&2
    exit 1
}

# ask QUESTION: yes with --yes; no without a terminal to ask on.
ask() {
    [ "$ASSUME_YES" = 1 ] && return 0
    if ! { : </dev/tty; } 2>/dev/null; then
        echo "$1 [skipped: no terminal; pass --yes]"
        return 1
    fi
    printf '%s [y/N] ' "$1"
    local answer
    read -r answer </dev/tty || return 1
    case $answer in y | Y | yes | s | S | sim) return 0 ;; *) return 1 ;; esac
}

# remove_openvino: undoes setup-npu.sh's OpenVINO install. Its ld.so.conf.d
# file marks an install of ours; without it nothing is touched, so an
# OpenVINO installed some other way stays.
remove_openvino() {
    local root=${SMART_COMMIT_SYSTEM_ROOT:-}
    local sudo=${SMART_COMMIT_SUDO-sudo}
    local conf=$root/etc/ld.so.conf.d/smart-commit-openvino.conf
    local link=$root/opt/intel/openvino target=
    if [ ! -e "$conf" ]; then
        say "no OpenVINO installed by smart-commit; nothing to remove"
        return
    fi
    if [ -L "$link" ]; then
        target=$(readlink "$link")
        case $target in
            /opt/intel/openvino_* | "$root"/opt/intel/openvino_*) target=$root/opt/intel/${target##*/} ;;
            *) target= ;;
        esac
    fi
    say "OpenVINO installed by smart-commit's setup:"
    echo "  $conf"
    [ -L "$link" ] && echo "  $link"
    [ -n "$target" ] && echo "  $target"
    ask "Remove it (uses sudo)?" || return 0
    $sudo rm -f "$conf"
    [ -L "$link" ] && $sudo rm -f "$link"
    [ -n "$target" ] && $sudo rm -rf "$target"
    $sudo rmdir "$root/opt/intel" 2>/dev/null || true
    # Only the real system's linker cache knows these libraries.
    [ -n "$root" ] || $sudo ldconfig
    say "removed OpenVINO"
}

if [ "$UNINSTALL" = 1 ]; then
    remove_release
    say "removed smart-commit from $PREFIX"
    data=${XDG_DATA_HOME:-$HOME/.local/share}/smart-commit
    cache=${XDG_CACHE_HOME:-$HOME/.cache}/smart-commit
    config=${XDG_CONFIG_HOME:-$HOME/.config}/smart-commit
    if [ "$PURGE" = 1 ]; then
        rm -rf "${data:?}" "${cache:?}" "${config:?}"
        say "removed the models, llama.cpp, the benchmark, the caches and the configuration"
    else
        echo "Kept (--purge removes them):"
        echo "  models and benchmark: $data"
        echo "  compiled-model cache: $cache"
        echo "  configuration:        $config"
    fi
    if [ "$SYSTEM" = 1 ]; then
        remove_openvino
    fi
    echo "Kept: driver packages (Intel NPU driver, Level Zero, Intel's compute runtime,"
    echo "Vulkan drivers) and render group membership; other programs may use them."
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
new=$(cat "$src/share/smart-commit/VERSION")
old=$(cat "$PREFIX/share/smart-commit/VERSION" 2>/dev/null || true)
if [ "$UPDATE" = 1 ] && [ "$new" = "$old" ]; then
    say "smart-commit $old is already the latest release"
    exit 0
fi
remove_release
mkdir -p "$PREFIX/bin" "$PREFIX/share/smart-commit" "$PREFIX/share/doc"
install -m 755 "$src/bin/smart-commit" "$PREFIX/bin/smart-commit"
for entry in "${RELEASE_FILES[@]}"; do
    cp -R "$src/share/smart-commit/$entry" "$PREFIX/share/smart-commit/$entry"
done
cp -R "$src/share/doc/smart-commit" "$PREFIX/share/doc/smart-commit"
say "installed $("$PREFIX/bin/smart-commit" --version) in $PREFIX"
if [ "$UPDATE" = 1 ]; then
    [ -n "$old" ] && say "updated from $old"
    # The new binary looks for the llama.cpp build its release pins; it
    # does nothing when that build is already there.
    if [ -d "${XDG_DATA_HOME:-$HOME/.local/share}/smart-commit/llama.cpp" ]; then
        "$PREFIX/bin/smart-commit" setup --llamacpp-only
    fi
fi

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "note: $PREFIX/bin is not on your PATH; add it to your shell's profile." ;;
esac

if [ "$SETUP" = 1 ]; then
    # With a terminal, `smart-commit setup` opens the full-screen setup (or
    # asks its questions, with --yes it asks none); with curl | bash, stdin is
    # the pipe, so it is given the terminal instead.
    if { : </dev/tty; } 2>/dev/null; then
        "$PREFIX/bin/smart-commit" setup "${setup_args[@]}" </dev/tty
    else
        "$PREFIX/bin/smart-commit" setup "${setup_args[@]}"
    fi
elif [ "$UPDATE" = 0 ]; then
    echo "Run \`smart-commit setup\` to set up the devices and models."
fi

#!/usr/bin/env bash
# Installs what the NPU backend needs on Ubuntu 24.04 (and derivatives such
# as Pop!_OS), with every artifact pinned by version and SHA-256:
#
#   - intel/linux-npu-driver 1.38.0 (user-space driver + NPU compiler)
#   - libze1 1.32.0 (Level Zero loader), the version that driver release
#     was validated with
#   - OpenVINO GenAI 2026.3.1 archive (runtime + GenAI + tokenizers),
#     also the version validated with the driver, in /opt/intel
#   - /etc/ld.so.conf.d/npu-commit-openvino.conf, so its libraries resolve
#     without LD_LIBRARY_PATH
#   - membership of the `render` group, which owns /dev/accel/accel0
#
# Run as your own user; it calls sudo where needed:
#
#   scripts/setup-npu.sh [--openvino-only]
#
# --openvino-only installs just OpenVINO GenAI (for the CPU and Intel GPUs),
# for machines without an NPU.
#
# Then log out and back in (for the group), and run scripts/check-npu.sh.
set -euo pipefail

OPENVINO_ONLY=0
case ${1:-} in
    --openvino-only) OPENVINO_ONLY=1 ;;
    "") ;;
    *)
        echo "unknown option: $1" >&2
        exit 2
        ;;
esac

NPU_DRIVER_URL=https://github.com/intel/linux-npu-driver/releases/download/v1.38.0/linux-npu-driver-v1.38.0.20260910-34487311128-ubuntu2404.tar.gz
NPU_DRIVER_SHA256=1efcd4b60c22abee751d8f2705962cbcc2a569de45c7e0e670cf08afbfcdc1d2

# From the snapshot the driver release notes point to; snapshots never change.
LIBZE1_URL='https://snapshot.ppa.launchpadcontent.net/kobuk-team/intel-graphics/ubuntu/20260830T100000Z/pool/main/l/level-zero-loader/libze1_1.32.0-1~24.04~ppa1_amd64.deb'
LIBZE1_SHA256=d4001dc474d3cb74ded50a449b9f646cfdfbb31e69e70b8af38f2d18273dfce1

GENAI_URL=https://storage.openvinotoolkit.org/repositories/openvino_genai/packages/2026.3.1/linux/openvino_genai_ubuntu24_2026.3.1.0_x86_64.tar.gz
GENAI_SHA256=be17c328d9ee313424e364e6ca43108b55e5a2057549a0dc9edcfd78acda224f
GENAI_DIR=openvino_genai_ubuntu24_2026.3.1.0_x86_64
# /opt/intel/openvino is one of the places the openvino-rs crates look in,
# so no environment variable is needed (git hooks from GUI clients do not
# see the shell's).
INSTALL_ROOT=/opt/intel
INSTALL_DIR=$INSTALL_ROOT/openvino_2026.3.1

# shellcheck source=/dev/null
. /etc/os-release
if [ "${VERSION_ID:-}" != "24.04" ]; then
    echo "warning: tested on Ubuntu 24.04; this is ${PRETTY_NAME:-unknown}" >&2
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# fetch URL SHA256 FILE: downloads and refuses anything with another checksum.
fetch() {
    echo "==> downloading $(basename "$3")"
    curl -fL --retry 3 -o "$work/$3" "$1"
    echo "$2  $work/$3" | sha256sum --check --quiet -
}

install_npu_driver() {
    fetch "$NPU_DRIVER_URL" "$NPU_DRIVER_SHA256" npu-driver.tar.gz
    fetch "$LIBZE1_URL" "$LIBZE1_SHA256" libze1.deb

    echo "==> installing libtbb12 (needed by the NPU compiler)"
    sudo apt-get update -qq
    sudo apt-get install -y libtbb12

    echo "==> installing the NPU driver 1.38.0"
    # Older versions go first, as the release notes ask.
    local package
    for package in intel-driver-compiler-npu intel-fw-npu intel-level-zero-npu intel-level-zero-npu-dbgsym; do
        if dpkg-query -W "$package" >/dev/null 2>&1; then
            sudo dpkg --purge --force-remove-reinstreq "$package"
        fi
    done
    mkdir "$work/npu-driver"
    tar -xzf "$work/npu-driver.tar.gz" -C "$work/npu-driver"
    # Only the .deb packages; the .ddeb is debug symbols.
    sudo dpkg -i "$work"/npu-driver/*.deb

    echo "==> installing libze1 1.32.0"
    sudo dpkg -i "$work/libze1.deb"
}

fetch "$GENAI_URL" "$GENAI_SHA256" genai.tar.gz
if [ "$OPENVINO_ONLY" = 0 ]; then
    install_npu_driver
fi

echo "==> installing OpenVINO GenAI 2026.3.1 in $INSTALL_DIR"
tar -xzf "$work/genai.tar.gz" -C "$work"
sudo mkdir -p "$INSTALL_ROOT"
sudo rm -rf "${INSTALL_DIR:?}"
sudo mv "$work/$GENAI_DIR" "$INSTALL_DIR"
sudo ln -sfn "$INSTALL_DIR" "$INSTALL_ROOT/openvino"
# The archive's libraries have no RUNPATH and expect setupvars.sh to set
# LD_LIBRARY_PATH, which processes started outside a shell (git hooks run by
# GUI clients) never get. Registering them with the dynamic linker instead
# makes them resolve everywhere.
printf '%s\n' "$INSTALL_ROOT/openvino/runtime/lib/intel64" "$INSTALL_ROOT/openvino/runtime/3rdparty/tbb/lib" |
    sudo tee /etc/ld.so.conf.d/npu-commit-openvino.conf >/dev/null
sudo ldconfig

if [ "$OPENVINO_ONLY" = 1 ]; then
    echo
    echo "Done: OpenVINO GenAI is installed (CPU; Intel GPUs also need scripts/setup-gpu.sh)."
    exit 0
fi

echo "==> adding $USER to the render group"
sudo gpasswd -a "$USER" render

echo
echo "Done. Log out and back in (or reboot) so the render group applies,"
echo "then run scripts/check-npu.sh."

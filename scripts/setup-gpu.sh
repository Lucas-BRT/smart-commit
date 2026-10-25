#!/usr/bin/env bash
# Installs the system drivers the GPUs of this machine need, per vendor and
# kernel driver, and nothing for vendors that are not there:
#
#   Intel  (i915/xe)  Mesa ANV (vulkan) + Intel's compute runtime for
#                     OpenVINO (intel-gpu: intel-opencl-icd & co., pinned,
#                     from the PPA snapshot the NPU driver release was
#                     validated with, checked against SHA-256)
#   AMD    (amdgpu)   Mesa RADV (vulkan)
#   NVIDIA (nvidia)   the proprietary driver's own Vulkan ICD; if missing,
#                     libnvidia-gl of the *same* version as the running driver
#                     (the driver itself is never changed)
#   NVIDIA (nouveau)  Mesa NVK (vulkan; Turing / GTX 16xx or newer)
#
# llama.cpp itself, which runs the vulkan device, is `smart-commit setup`'s job;
# OpenVINO, which intel-gpu also needs, is setup-npu.sh --openvino-only's.
#
#   scripts/setup-gpu.sh [--dry-run] [--yes]
#
#   --dry-run  print what would be done, change nothing
#   --yes      do not ask before using sudo
#
# System packages need apt (Ubuntu, Debian, Pop!_OS); elsewhere the script
# says which packages to install and still sets up the user-space parts.
# SMART_COMMIT_SYSFS overrides /sys, SMART_COMMIT_VULKAN_ICD_DIRS the Vulkan ICD
# directories and SMART_COMMIT_DPKG_QUERY the installed-package check, for
# tests.
set -euo pipefail

# Intel compute runtime for OpenVINO's GPU plugin: kobuk-team/intel-graphics
# snapshot 20260830T100000Z, as referenced by linux-npu-driver 1.38.0.
INTEL_BASE=https://snapshot.ppa.launchpadcontent.net/kobuk-team/intel-graphics/ubuntu/20260830T100000Z/pool/main
INTEL_DEBS=(
    "i/intel-compute-runtime/intel-opencl-icd_26.27.39122.14-1~24.04~ppa1_amd64.deb 7739e7196499a9da23f9285365596afe9eeda48e28afe2908b4013f2aa890a45"
    "i/intel-graphics-compiler/libigc2_2.38.5-1~24.04_amd64.deb ef695d1227814d80de9ccb5230535fec0818b8109f7f069d9b8d2a31998d744c"
    "i/intel-graphics-compiler/libigdfcl2_2.38.5-1~24.04_amd64.deb 913e7334e2f3470ce4667e07f10f47a8201e5cea43413bd867205945cfeff002"
    "i/intel-gmmlib/libigdgmm12_22.10.0-1~24.04~ppa1_amd64.deb 63fe9e124c3394193226c6cb8398dcabdc9306b5a10f63668118f7c799f5d792"
)

SYSFS=${SMART_COMMIT_SYSFS:-/sys}
DRY_RUN=0
ASSUME_YES=0
for arg in "$@"; do
    case $arg in
        --dry-run) DRY_RUN=1 ;;
        --yes) ASSUME_YES=1 ;;
        -h | --help)
            sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown option: $arg" >&2
            exit 2
            ;;
    esac
done

say() { echo "==> $*"; }
warn() { echo "warning: $*" >&2; }

# run CMD...: runs it, or prints it with --dry-run.
run() {
    if [ "$DRY_RUN" = 1 ]; then
        echo "would run: $*"
    else
        "$@"
    fi
}

confirmed_sudo=0
# sudo_run CMD...: asks once (unless --yes), then runs with sudo.
sudo_run() {
    if [ "$DRY_RUN" = 0 ] && [ "$ASSUME_YES" = 0 ] && [ "$confirmed_sudo" = 0 ]; then
        printf 'The next steps use sudo to install system packages. Continue? [y/N] '
        read -r answer </dev/tty || answer=n
        case $answer in
            y | Y | yes) confirmed_sudo=1 ;;
            *)
                echo "Stopped; nothing else was changed."
                exit 1
                ;;
        esac
    fi
    run sudo "$@"
}

# fetch URL SHA256 DEST: downloads and refuses another checksum.
fetch() {
    if [ "$DRY_RUN" = 1 ]; then
        echo "would download: $1 -> $3 (sha256 $2)"
        return
    fi
    mkdir -p "$(dirname "$3")"
    curl -fL --retry 3 -o "$3.partial" "$1"
    echo "$2  $3.partial" | sha256sum --check --quiet -
    mv "$3.partial" "$3"
}

have_apt() { command -v apt-get >/dev/null; }

# installed PACKAGE [VERSION]: whether dpkg has it (at that version).
installed() {
    local status
    # The format is dpkg-query's, not the shell's.
    # shellcheck disable=SC2016
    status=$(${SMART_COMMIT_DPKG_QUERY:-dpkg-query} -W -f='${Status} ${Version}' "$1" 2>/dev/null) || return 1
    case $status in
        "install ok installed ${2:-}"*) return 0 ;;
        *) return 1 ;;
    esac
}

apt_packages=()
need_apt() { apt_packages+=("$@"); }

# --- detection -----------------------------------------------------------------

# list_gpus: "vendor driver slot" for every display controller (PCI class
# 0x03), from sysfs, in slot order.
list_gpus() {
    local dev driver
    for dev in "$SYSFS"/bus/pci/devices/*; do
        [ -r "$dev/class" ] || continue
        case $(cat "$dev/class") in 0x03*) ;; *) continue ;; esac
        driver=none
        if [ -L "$dev/driver" ]; then
            driver=$(basename "$(readlink "$dev/driver")")
        fi
        echo "$(cat "$dev/vendor") $driver $(basename "$dev")"
    done
}

nvidia_driver_version() {
    cat "$SYSFS/module/nvidia/version" 2>/dev/null || true
}

nvidia_vulkan_icd_present() {
    local dir
    for dir in ${SMART_COMMIT_VULKAN_ICD_DIRS:-/usr/share/vulkan/icd.d /etc/vulkan/icd.d}; do
        [ -e "$dir/nvidia_icd.json" ] && return 0
    done
    return 1
}

# --- per vendor --------------------------------------------------------------

intel_runtime=0
vulkan=0

setup_intel() {
    say "Intel GPU ($1, driver $2): Vulkan (Mesa ANV) and Intel's compute runtime for OpenVINO"
    case $2 in
        i915 | xe) ;;
        *) warn "the Intel GPU at $1 has no i915/xe kernel driver bound; it will not be usable" ;;
    esac
    intel_runtime=1
    vulkan=1
    need_apt mesa-vulkan-drivers
}

setup_amd() {
    say "AMD GPU ($1, driver $2): Vulkan (Mesa RADV)"
    case $2 in
        amdgpu) ;;
        radeon) warn "$1 uses the old radeon driver; RADV needs amdgpu (GCN 1.2 or newer)" ;;
        *) warn "the AMD GPU at $1 has no amdgpu kernel driver bound" ;;
    esac
    vulkan=1
    need_apt mesa-vulkan-drivers
}

setup_nvidia() {
    case $2 in
        nvidia)
            local version
            version=$(nvidia_driver_version)
            say "NVIDIA GPU ($1, proprietary driver ${version:-unknown}): its own Vulkan ICD"
            vulkan=1
            if nvidia_vulkan_icd_present; then
                echo "    the NVIDIA Vulkan ICD is installed"
            elif [ -n "$version" ]; then
                # The userspace must match the kernel module exactly; only the
                # package of the running driver's branch is installed.
                need_apt "libnvidia-gl-${version%%.*}"
            else
                warn "cannot tell the NVIDIA driver version; install its Vulkan support (libnvidia-gl-<version>)"
            fi
            ;;
        nouveau)
            say "NVIDIA GPU ($1, open-source nouveau driver): Vulkan (Mesa NVK)"
            echo "    NVK needs a Turing (GTX 16xx / RTX 20xx) or newer GPU and Mesa 24.1+"
            vulkan=1
            need_apt mesa-vulkan-drivers
            ;;
        *)
            warn "the NVIDIA GPU at $1 has no driver bound; install the proprietary driver \
(e.g. sudo ubuntu-drivers install) or use nouveau, then run this again"
            ;;
    esac
}

# --- main --------------------------------------------------------------------

gpus=$(list_gpus)
if [ -z "$gpus" ]; then
    echo "No GPU found: nothing to install here; smart-commit will use the CPU (or an Intel NPU)."
    exit 0
fi
while read -r vendor driver slot; do
    case $vendor in
        0x8086) setup_intel "$slot" "$driver" ;;
        0x1002) setup_amd "$slot" "$driver" ;;
        0x10de) setup_nvidia "$slot" "$driver" ;;
        *) warn "GPU $slot from vendor $vendor is not supported" ;;
    esac
done <<<"$gpus"

if [ "$vulkan" = 1 ]; then
    need_apt libvulkan1
fi

if [ "${#apt_packages[@]}" -gt 0 ]; then
    # Deduplicated, in a stable order, without what is already installed.
    mapfile -t apt_packages < <(printf '%s\n' "${apt_packages[@]}" | sort -u)
    missing=()
    for package in "${apt_packages[@]}"; do
        installed "$package" || missing+=("$package")
    done
    apt_packages=("${missing[@]}")
fi
if [ "${#apt_packages[@]}" -gt 0 ]; then
    if have_apt || [ "$DRY_RUN" = 1 ]; then
        say "system packages: ${apt_packages[*]}"
        sudo_run apt-get update -qq
        sudo_run apt-get install -y "${apt_packages[@]}"
    else
        warn "no apt here; install these (or your distribution's equivalent): ${apt_packages[*]}"
    fi
fi

if [ "$intel_runtime" = 1 ] && installed intel-opencl-icd 26.27.39122.14; then
    say "the Intel compute runtime 26.27.39122.14 is installed"
elif [ "$intel_runtime" = 1 ]; then
    if have_apt || [ "$DRY_RUN" = 1 ]; then
        say "Intel's compute runtime 26.27.39122.14 for OpenVINO (pinned)"
        work=$(mktemp -d)
        trap 'rm -rf "$work"' EXIT
        debs=()
        for entry in "${INTEL_DEBS[@]}"; do
            read -r path sha <<<"$entry"
            fetch "$INTEL_BASE/$path" "$sha" "$work/$(basename "$path")"
            debs+=("$work/$(basename "$path")")
        done
        # dpkg runs ldconfig, which creates the libigc.so.2 & co. links the
        # runtime opens; without them it fails with CL_OUT_OF_HOST_MEMORY.
        sudo_run dpkg -i "${debs[@]}"
        sudo_run gpasswd -a "$USER" render
    else
        warn "no apt here; install Intel's compute runtime (intel-opencl-icd) to use the GPU through OpenVINO"
    fi
fi

echo
echo "Done. If you were added to the render group, log out and back in."
echo "The vulkan device also needs llama.cpp (\`smart-commit setup\` installs it); then"
echo "\`smart-commit bench\` measures every device and picks the fastest."

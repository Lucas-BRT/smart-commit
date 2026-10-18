#!/usr/bin/env bash
# The hardware of this machine, as smart-commit sees it: the CPU, every GPU
# and NPU, their vendor and kernel driver, and which smart-commit devices
# each one gives.
#
#   scripts/hardware.sh            prints the inventory
#   . scripts/hardware.sh          defines the functions below, for the
#                                  other setup scripts
#
# Everything comes from sysfs and /proc/cpuinfo; nothing needs root.
# SMART_COMMIT_SYSFS overrides /sys (lspci, which reads the real machine, is
# then not used), SMART_COMMIT_CPUINFO /proc/cpuinfo and
# SMART_COMMIT_OPENVINO_DIR /opt/intel/openvino, for tests.

SYSFS=${SMART_COMMIT_SYSFS:-/sys}
CPUINFO=${SMART_COMMIT_CPUINFO:-/proc/cpuinfo}
OPENVINO_DIR=${SMART_COMMIT_OPENVINO_DIR:-/opt/intel/openvino}

# cpu_field NAME: the first value of NAME in /proc/cpuinfo.
cpu_field() {
    sed -n "s/^$1[[:space:]]*: //p" "$CPUINFO" 2>/dev/null | head -1
}

# cpu_vendor: intel, amd or other.
cpu_vendor() {
    case $(cpu_field vendor_id) in
        GenuineIntel) echo intel ;;
        AuthenticAMD) echo amd ;;
        *) echo other ;;
    esac
}

cpu_model() {
    local model
    model=$(cpu_field 'model name')
    echo "${model:-unknown CPU}"
}

# pci_devices CLASS_PREFIX: prints "vendor device driver slot" for each PCI
# device whose class starts with CLASS_PREFIX (0x03 for display controllers).
pci_devices() {
    local dev class driver
    for dev in "$SYSFS"/bus/pci/devices/*; do
        [ -r "$dev/class" ] || continue
        class=$(cat "$dev/class")
        case $class in "$1"*) ;; *) continue ;; esac
        driver=none
        if [ -L "$dev/driver" ]; then
            driver=$(basename "$(readlink "$dev/driver")")
        fi
        echo "$(cat "$dev/vendor") $(cat "$dev/device" 2>/dev/null || echo 0x0000) $driver $(basename "$dev")"
    done
}

# list_gpus: "vendor driver slot" for every display controller.
list_gpus() {
    pci_devices 0x03 | while read -r vendor _ driver slot; do
        echo "$vendor $driver $slot"
    done
}

gpu_count() { list_gpus | wc -l; }

has_intel_gpu() { list_gpus | grep -q '^0x8086 '; }

# has_intel_npu: Intel's NPUs are processing accelerators (class 0x1200);
# the intel_vpu module is the fallback for kernels that list them otherwise.
has_intel_npu() {
    pci_devices 0x1200 | grep -q '^0x8086 ' || [ -d "$SYSFS/module/intel_vpu" ]
}

# has_amd_npu: Ryzen AI (XDNA) NPUs, which smart-commit does not use.
has_amd_npu() {
    pci_devices 0x11 | grep -qE '^0x1022 (0x1502|0x17f0) '
}

# needs_openvino: only Intel's NPU and GPUs need OpenVINO.
needs_openvino() { has_intel_npu || has_intel_gpu; }

openvino_installed() { [ -e "$OPENVINO_DIR/runtime/lib/intel64/libopenvino_genai_c.so" ]; }

vendor_name() {
    case $1 in
        0x8086) echo Intel ;;
        0x1002 | 0x1022) echo AMD ;;
        0x10de) echo NVIDIA ;;
        *) echo "vendor $1" ;;
    esac
}

# pci_name SLOT VENDOR DEVICE: the marketing name from lspci when this is the
# real machine, else the PCI IDs.
pci_name() {
    local name=
    if [ "$SYSFS" = /sys ] && command -v lspci >/dev/null; then
        name=$(lspci -vmm -s "$1" 2>/dev/null | sed -n 's/^Device:[[:space:]]*//p' | head -1)
    fi
    echo "${name:-$(vendor_name "$2") device ${3#0x}}"
}

# gpu_devices VENDOR DRIVER: the smart-commit devices a GPU gives.
gpu_devices() {
    case $1:$2 in
        0x8086:i915 | 0x8086:xe) echo "intel-gpu (OpenVINO), vulkan (Mesa ANV)" ;;
        0x1002:amdgpu) echo "vulkan (Mesa RADV)" ;;
        0x10de:nvidia) echo "vulkan (the NVIDIA driver's Vulkan ICD)" ;;
        0x10de:nouveau) echo "vulkan (Mesa NVK, Turing or newer)" ;;
        *:none) echo "none until a kernel driver is bound" ;;
        0x1002:radeon) echo "none: the old radeon driver has no Vulkan; amdgpu does" ;;
        *) echo "none: not supported" ;;
    esac
}

# print_inventory: what this machine has, and what smart-commit can use.
print_inventory() {
    local vendor device driver slot found=0 version
    echo "  CPU  $(cpu_model) ($(uname -m))"
    case $(uname -m) in
        x86_64) echo "       -> cpu (llama.cpp); openvino-cpu where OpenVINO is installed" ;;
        *) echo "       -> none: smart-commit runs on x86_64 only" ;;
    esac
    while read -r vendor device driver slot; do
        [ -n "$vendor" ] || continue
        found=1
        version=
        if [ "$driver" = nvidia ]; then
            version=" $(cat "$SYSFS/module/nvidia/version" 2>/dev/null || echo '(unknown version)')"
        fi
        echo "  GPU  $(pci_name "$slot" "$vendor" "$device") [$slot], driver $driver$version"
        echo "       -> $(gpu_devices "$vendor" "$driver")"
    done < <(pci_devices 0x03)
    [ "$found" = 1 ] || echo "  GPU  none"
    if has_intel_npu; then
        echo "  NPU  Intel NPU"
        echo "       -> npu (OpenVINO)"
    elif has_amd_npu; then
        echo "  NPU  AMD Ryzen AI (XDNA)"
        echo "       -> none: not supported"
    else
        echo "  NPU  none"
    fi
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -euo pipefail
    print_inventory
fi

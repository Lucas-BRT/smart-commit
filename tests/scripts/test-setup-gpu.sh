#!/usr/bin/env bash
# Runs scripts/setup-gpu.sh --dry-run against fake /sys trees, one per
# hardware scenario, and checks what it would do. No GPU, network or sudo.
#
#   tests/scripts/test-setup-gpu.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT=$ROOT/scripts/setup-gpu.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

# gpu SYSFS SLOT VENDOR DRIVER: adds a display controller to a fake tree.
gpu() {
    local dev=$1/bus/pci/devices/$2
    mkdir -p "$dev" "$1/bus/pci/drivers/$4"
    echo 0x030000 >"$dev/class"
    echo "$3" >"$dev/vendor"
    if [ "$4" != none ]; then
        ln -s "../../../bus/pci/drivers/$4" "$dev/driver"
    fi
}

# other SYSFS SLOT: a non-display PCI device, which must be ignored.
other() {
    local dev=$1/bus/pci/devices/$2
    mkdir -p "$dev"
    echo 0x020000 >"$dev/class"
    echo 0x8086 >"$dev/vendor"
}

# scenario NAME SETUP: builds a tree with SETUP and dry-runs the script.
scenario() {
    local name=$1 setup=$2
    local sysfs=$work/$name
    mkdir -p "$sysfs/bus/pci/devices" "$work/icd-$name"
    $setup "$sysfs"
    # Nothing counts as installed, so the plan does not depend on the machine.
    SMART_COMMIT_SYSFS=$sysfs SMART_COMMIT_VULKAN_ICD_DIRS=$work/icd-$name SMART_COMMIT_DPKG_QUERY=false \
        XDG_DATA_HOME=$work/data-$name bash "$SCRIPT" --dry-run >"$work/$name.out" 2>&1 || {
        echo "FAIL  $name: exited with $?"
        cat "$work/$name.out"
        failures=$((failures + 1))
    }
}

expect() {
    if grep -qF -- "$2" "$work/$1.out"; then
        echo "ok    $1: $2"
    else
        echo "FAIL  $1: expected: $2"
        sed 's/^/        /' "$work/$1.out"
        failures=$((failures + 1))
    fi
}

refute() {
    if grep -qF -- "$2" "$work/$1.out"; then
        echo "FAIL  $1: unexpected: $2"
        failures=$((failures + 1))
    else
        echo "ok    $1: no $2"
    fi
}

intel() { gpu "$1" 0000:00:02.0 0x8086 i915; other "$1" 0000:00:14.0; }
amd() { gpu "$1" 0000:03:00.0 0x1002 amdgpu; }
nvidia_proprietary() {
    gpu "$1" 0000:01:00.0 0x10de nvidia
    mkdir -p "$1/module/nvidia"
    echo 550.144.03 >"$1/module/nvidia/version"
}
nvidia_with_icd() {
    nvidia_proprietary "$1"
    touch "$work/icd-nvidia_with_icd/nvidia_icd.json"
}
nouveau() { gpu "$1" 0000:01:00.0 0x10de nouveau; }
nvidia_no_driver() { gpu "$1" 0000:01:00.0 0x10de none; }
hybrid() { intel "$1"; nvidia_proprietary "$1"; }
nothing() { other "$1" 0000:00:14.0; }

scenario intel intel
expect intel "Intel GPU (0000:00:02.0, driver i915)"
expect intel "intel-opencl-icd_26.27.39122.14"
expect intel "would run: sudo dpkg -i"
expect intel "would run: sudo apt-get install -y libvulkan1 mesa-vulkan-drivers"
expect intel "Intel's compute runtime 26.27.39122.14 for OpenVINO"
# llama.cpp is setup-llamacpp.sh's job.
refute intel "llama-b11381"
refute intel ".gguf"

scenario amd amd
expect amd "AMD GPU (0000:03:00.0, driver amdgpu): Vulkan (Mesa RADV)"
expect amd "apt-get install -y libvulkan1 mesa-vulkan-drivers"
refute amd "intel-opencl-icd"
refute amd "OpenVINO"

scenario nvidia_proprietary nvidia_proprietary
expect nvidia_proprietary "proprietary driver 550.144.03"
expect nvidia_proprietary "apt-get install -y libnvidia-gl-550 libvulkan1"
refute nvidia_proprietary "mesa-vulkan-drivers"
refute nvidia_proprietary "OpenVINO"
refute nvidia_proprietary "intel-opencl-icd"

scenario nvidia_with_icd nvidia_with_icd
expect nvidia_with_icd "the NVIDIA Vulkan ICD is installed"
refute nvidia_with_icd "libnvidia-gl"

scenario nouveau nouveau
expect nouveau "open-source nouveau driver): Vulkan (Mesa NVK)"
expect nouveau "Turing"
expect nouveau "mesa-vulkan-drivers"

scenario nvidia_no_driver nvidia_no_driver
expect nvidia_no_driver "has no driver bound"
refute nvidia_no_driver "llama-b11381"

scenario hybrid hybrid
expect hybrid "Intel GPU"
expect hybrid "NVIDIA GPU"
expect hybrid "apt-get install -y libnvidia-gl-550 libvulkan1 mesa-vulkan-drivers"

# What is installed is skipped.
already() { amd "$1"; }
fake_dpkg=$work/fake-dpkg-query
printf '#!/bin/sh\necho "install ok installed 26.1"\n' >"$fake_dpkg"
chmod +x "$fake_dpkg"
mkdir -p "$work/already/bus/pci/devices" "$work/icd-already"
amd "$work/already"
SMART_COMMIT_SYSFS=$work/already SMART_COMMIT_VULKAN_ICD_DIRS=$work/icd-already SMART_COMMIT_DPKG_QUERY=$fake_dpkg \
    XDG_DATA_HOME=$work/data-already bash "$SCRIPT" --dry-run >"$work/already.out" 2>&1
refute already "apt-get install"

scenario nothing nothing
expect nothing "No GPU found: nothing to install here"
refute nothing "would run"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all scenarios passed"

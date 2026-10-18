#!/usr/bin/env bash
# Runs scripts/setup.sh without drivers on fake machines (a fake /sys and
# /proc/cpuinfo), with a stub smart-commit and
# stub setup-llamacpp.sh and export-model.sh: it lists the hardware, plans
# only what that hardware needs, exports the OpenVINO model only for Intel
# devices and runs the benchmark. No network, sudo or hardware.
#
#   tests/scripts/test-setup.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

# The installed layout: prefix/bin and prefix/share/smart-commit/scripts.
prefix=$work/prefix
scripts=$prefix/share/smart-commit/scripts
mkdir -p "$prefix/bin" "$scripts/export"
cp "$ROOT"/scripts/{setup.sh,hardware.sh,setup-npu.sh,setup-gpu.sh,check-npu.sh} "$scripts/"
printf '#!/bin/sh\necho "stub smart-commit $*"\n' >"$prefix/bin/smart-commit"
printf '#!/bin/sh\necho "stub setup-llamacpp $*"\n' >"$scripts/setup-llamacpp.sh"
# Not exported yet: --check fails.
cat >"$scripts/export-model.sh" <<'STUB'
#!/bin/sh
[ "$1" = --check ] && exit 1
echo "stub export-model $*"
STUB
chmod +x "$prefix/bin/smart-commit" "$scripts/setup-llamacpp.sh" "$scripts/export-model.sh"

# pci SYSFS SLOT CLASS VENDOR DEVICE DRIVER: adds a PCI device.
pci() {
    local dev=$1/bus/pci/devices/$2
    mkdir -p "$dev" "$1/bus/pci/drivers/$6"
    echo "$3" >"$dev/class"
    echo "$4" >"$dev/vendor"
    echo "$5" >"$dev/device"
    [ "$6" = none ] || ln -s "../../../bus/pci/drivers/$6" "$dev/driver"
}

# cpu SYSFS VENDOR_ID MODEL: the CPU, in the tree's cpuinfo.
cpu() { printf 'vendor_id\t: %s\nmodel name\t: %s\n' "$2" "$3" >"$1/cpuinfo"; }

amd_nvidia() {
    cpu "$1" AuthenticAMD "AMD Ryzen 5 5500"
    pci "$1" 0000:01:00.0 0x030000 0x10de 0x2507 nvidia
    mkdir -p "$1/module/nvidia"
    echo 595.104.02 >"$1/module/nvidia/version"
}
all_intel() {
    cpu "$1" GenuineIntel "Intel(R) Core(TM) Ultra 5 225H"
    pci "$1" 0000:00:02.0 0x030000 0x8086 0x7d51 xe
    pci "$1" 0000:00:0b.0 0x120000 0x8086 0xad1d intel_vpu
}
amd_only() {
    cpu "$1" AuthenticAMD "AMD Ryzen 7 8845HS"
    # A Ryzen AI NPU, which smart-commit does not use.
    pci "$1" 0000:c4:00.1 0x118000 0x1022 0x1502 amdxdna
}
old_amd() {
    cpu "$1" AuthenticAMD "AMD FX-8350"
    pci "$1" 0000:01:00.0 0x030000 0x1002 0x6798 radeon
    pci "$1" 0000:02:00.0 0x030000 0x10de 0x1c82 none
}

# scenario NAME SETUP [OPTION...]: builds a fake machine with SETUP and runs
# setup.sh --no-drivers [OPTION...].
scenario() {
    local name=$1 setup=$2
    shift 2
    local machine=$work/$name
    mkdir -p "$machine/sys/bus/pci/devices"
    $setup "$machine/sys"
    SMART_COMMIT_SYSFS=$machine/sys SMART_COMMIT_CPUINFO=$machine/sys/cpuinfo \
        SMART_COMMIT_OPENVINO_DIR=$machine/no-openvino SMART_COMMIT_DPKG_QUERY=false \
        XDG_DATA_HOME=$machine/data \
        bash "$scripts/setup.sh" --no-drivers "$@" </dev/null >"$work/$name.out" 2>&1 || {
        echo "FAIL  $name: exited with $?"
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
        sed 's/^/        /' "$work/$1.out"
        failures=$((failures + 1))
    else
        echo "ok    $1: no $2"
    fi
}

# The hardware comes before the plan, and the plan before any step.
in_order() {
    local first second
    first=$(grep -nF -- "$2" "$work/$1.out" | head -1 | cut -d: -f1)
    second=$(grep -nF -- "$3" "$work/$1.out" | head -1 | cut -d: -f1)
    if [ -n "$first" ] && [ -n "$second" ] && [ "$first" -lt "$second" ]; then
        echo "ok    $1: \"$2\" before \"$3\""
    else
        echo "FAIL  $1: expected \"$2\" before \"$3\""
        failures=$((failures + 1))
    fi
}

scenario amd_nvidia amd_nvidia
expect amd_nvidia "CPU  AMD Ryzen 5 5500"
expect amd_nvidia "-> cpu (llama.cpp)"
expect amd_nvidia "GPU  NVIDIA device 2507 [0000:01:00.0], driver nvidia 595.104.02"
expect amd_nvidia "-> vulkan (the NVIDIA driver's Vulkan ICD)"
expect amd_nvidia "NPU  none"
expect amd_nvidia "NVIDIA GPU (0000:01:00.0, proprietary driver 595.104.02)"
expect amd_nvidia "not needed: OpenVINO, the Intel NPU driver and Intel's GPU runtime"
expect amd_nvidia "stub setup-llamacpp"
refute amd_nvidia "OpenVINO GenAI 2026.3.1"
refute amd_nvidia "OpenVINO model"
refute amd_nvidia "stub export-model"
in_order amd_nvidia "==> Hardware" "==> Plan"
in_order amd_nvidia "==> Plan" "stub setup-llamacpp"

scenario all_intel all_intel --yes
expect all_intel "CPU  Intel(R) Core(TM) Ultra 5 225H"
expect all_intel "-> intel-gpu (OpenVINO), vulkan (Mesa ANV)"
expect all_intel "NPU  Intel NPU"
expect all_intel "-> npu (OpenVINO)"
expect all_intel "Intel NPU: the NPU driver 1.38.0 and Level Zero 1.32.0"
expect all_intel "OpenVINO GenAI 2026.3.1 (sudo), for the Intel NPU or GPU"
expect all_intel "and the model exported for it here, from Hugging Face"
expect all_intel "Intel GPU (0000:00:02.0, driver xe)"
expect all_intel "stub export-model"
in_order all_intel "==> OpenVINO model" "stub export-model"

# Without --yes and without a terminal, the export is not started.
scenario intel_no_terminal all_intel
expect intel_no_terminal "Export the model for OpenVINO here"
expect intel_no_terminal "[skipped: no terminal; run setup.sh again to do it]"
refute intel_no_terminal "stub export-model"
refute all_intel "not needed"

scenario amd_only amd_only
expect amd_only "GPU  none"
expect amd_only "NPU  AMD Ryzen AI (XDNA)"
expect amd_only "-> none: not supported"
expect amd_only "not needed: OpenVINO"
expect amd_only "stub setup-llamacpp"
refute amd_only "OpenVINO model"
refute amd_only "stub export-model"

scenario old_amd old_amd
expect old_amd "-> none: the old radeon driver has no Vulkan; amdgpu does"
expect old_amd "-> none until a kernel driver is bound"
expect old_amd "has no driver bound"
expect old_amd "stub setup-llamacpp"

for name in amd_nvidia all_intel amd_only old_amd; do
    # The benchmark runs unless the render group was just added; both are fine.
    if grep -q "Benchmark postponed" "$work/$name.out"; then
        expect "$name" "smart-commit bench"
    else
        expect "$name" "stub smart-commit bench"
    fi
    expect "$name" "smart-commit hook install"
done

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all scenarios passed"

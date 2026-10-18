#!/usr/bin/env bash
# Checks, without changing anything, that the NPU stack installed by
# scripts/setup-npu.sh is in place and usable by the current user.
# Exits non-zero if anything is missing.
set -uo pipefail

failures=0
check() {
    local description=$1
    shift
    if "$@" >/dev/null 2>&1; then
        echo "ok    $description"
    else
        echo "FAIL  $description"
        failures=$((failures + 1))
    fi
}

device=/dev/accel/accel0
check "NPU device $device exists" test -e "$device"
check "intel_vpu kernel driver is loaded" test -d /sys/module/intel_vpu
check "$USER can read and write $device" test -r "$device" -a -w "$device"
check "intel-level-zero-npu 1.38.0 is installed" \
    sh -c "dpkg-query -W -f='\${Version}' intel-level-zero-npu | grep -q '^1.38.0'"
check "intel-driver-compiler-npu 1.38.0 is installed" \
    sh -c "dpkg-query -W -f='\${Version}' intel-driver-compiler-npu | grep -q '^1.38.0'"
check "libze1 1.32.0 is installed" \
    sh -c "dpkg-query -W -f='\${Version}' libze1 | grep -q '^1.32.0'"
check "libtbb12 is installed" dpkg-query -W libtbb12
check "OpenVINO GenAI 2026.3.1 is in /opt/intel/openvino" \
    grep -q '^2026.3.1' /opt/intel/openvino/runtime/version.txt
check "the GenAI C library is there" \
    test -e /opt/intel/openvino/runtime/lib/intel64/libopenvino_genai_c.so
check "the dynamic linker resolves the OpenVINO libraries" \
    sh -c "ldconfig -p | grep -q 'libopenvino_c.so'"

if [ "$failures" -gt 0 ]; then
    echo
    echo "$failures check(s) failed; see scripts/setup-npu.sh."
    if ! id -nG | grep -qw render && getent group render | grep -qw "$USER"; then
        echo "You are in the render group but this session is not: log out and back in."
    fi
    exit 1
fi
echo
echo "The NPU stack is ready."

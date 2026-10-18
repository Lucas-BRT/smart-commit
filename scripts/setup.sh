#!/usr/bin/env bash
# Guided setup, run by install.sh: finds the NPU and GPUs of this machine,
# offers to install what each needs, fetches the models, and benchmarks the
# devices so `model.device = auto` uses the fastest one.
#
#   setup.sh [--yes] [--no-drivers] [--no-bench]
#
#   --yes         answer yes to every question (sudo is still sudo)
#   --no-drivers  skip system drivers and runtimes (no sudo at all)
#   --no-bench    skip the benchmark
#
# Every step is optional and can be run again later on its own:
# setup-npu.sh, setup-gpu.sh, fetch-model.sh and `npu-commit bench`.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
SYSFS=${NPU_COMMIT_SYSFS:-/sys}
ASSUME_YES=0
DRIVERS=1
BENCH=1
for arg in "$@"; do
    case $arg in
        --yes) ASSUME_YES=1 ;;
        --no-drivers) DRIVERS=0 ;;
        --no-bench) BENCH=0 ;;
        *)
            echo "unknown option: $arg" >&2
            exit 2
            ;;
    esac
done

# The binary: installed next to share/, else on PATH, else this checkout's.
if [ -x "$HERE/../../../bin/npu-commit" ]; then
    NPU_COMMIT=$HERE/../../../bin/npu-commit
elif command -v npu-commit >/dev/null; then
    NPU_COMMIT=npu-commit
else
    NPU_COMMIT=$HERE/../target/release/npu-commit
fi

say() { printf '\n==> %s\n' "$*"; }

# ask QUESTION: yes with --yes; no without a terminal to ask on.
ask() {
    if [ "$ASSUME_YES" = 1 ]; then
        return 0
    fi
    if ! { : </dev/tty; } 2>/dev/null; then
        echo "$1 [skipped: no terminal; run setup.sh again to do it]"
        return 1
    fi
    printf '%s [y/N] ' "$1"
    local answer
    read -r answer </dev/tty || return 1
    case $answer in y | Y | yes | s | S | sim) return 0 ;; *) return 1 ;; esac
}

has_npu() { [ -e /dev/accel/accel0 ] || [ -d "$SYSFS/module/intel_vpu" ]; }

gpu_count() {
    local dev count=0
    for dev in "$SYSFS"/bus/pci/devices/*; do
        [ -r "$dev/class" ] || continue
        case $(cat "$dev/class") in 0x03*) count=$((count + 1)) ;; esac
    done
    echo "$count"
}

openvino_installed() { [ -e /opt/intel/openvino/runtime/lib/intel64/libopenvino_genai_c.so ]; }

say "Hardware"
if has_npu; then echo "  NPU: Intel NPU found"; else echo "  NPU: none"; fi
echo "  GPUs: $(gpu_count)"

if [ "$DRIVERS" = 1 ]; then
    if has_npu; then
        if "$HERE/check-npu.sh" >/dev/null 2>&1; then
            say "NPU: driver and OpenVINO already set up"
        elif ask "Install the NPU driver and OpenVINO (uses sudo)?"; then
            "$HERE/setup-npu.sh"
        fi
    elif ! openvino_installed; then
        if ask "Install OpenVINO, to run on the CPU (uses sudo)?"; then
            "$HERE/setup-npu.sh" --openvino-only
        fi
    fi
    if [ "$(gpu_count)" -gt 0 ]; then
        say "GPUs"
        "$HERE/setup-gpu.sh" --dry-run | grep '^==>' | sed 's/^==> /  /' || true
        if ask "Set up the GPUs as above (may use sudo)?"; then
            yes_flag=()
            [ "$ASSUME_YES" = 1 ] && yes_flag=(--yes)
            "$HERE/setup-gpu.sh" "${yes_flag[@]}"
        fi
    fi
fi

say "Models"
if ! "$HERE/fetch-model.sh"; then
    echo "  Could not download the OpenVINO model; export it locally with"
    echo "  $HERE/export-model.sh (needs uv and about 10 GB of disk)."
fi

if [ "$BENCH" = 1 ]; then
    if ! id -nG | grep -qw render && getent group render | grep -qw "$USER"; then
        say "Benchmark postponed"
        echo "  You were just added to the render group, which only applies after"
        echo "  logging out and back in. Then run: npu-commit bench"
    else
        say "Benchmark"
        "$NPU_COMMIT" bench || echo "  Nothing could run the model yet; see above."
    fi
fi

say "Done"
echo "  Generate a message:   git add -p && npu-commit"
echo "  Commit with it:       npu-commit --commit"
echo "  On every git commit:  npu-commit hook install"
echo "  Change the device:    npu-commit device use <npu|gpu|cpu|vulkan|auto>"

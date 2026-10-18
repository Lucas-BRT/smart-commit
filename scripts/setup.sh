#!/usr/bin/env bash
# Guided setup, run by install.sh and `smart-commit setup`. It lists the
# hardware of this machine first (CPU, GPUs, NPU, with vendor and driver),
# then plans what that hardware needs and only that, asks before each step
# that uses sudo, and benchmarks the devices so `model.device = auto` uses
# the fastest one:
#
#   always              llama.cpp + the GGUF model, no sudo: the cpu device
#                       (any x86_64) and the vulkan device (any GPU)
#   per GPU             its Vulkan driver (setup-gpu.sh); Intel GPUs also
#                       Intel's compute runtime, for intel-gpu
#   Intel NPU           the NPU driver and Level Zero (setup-npu.sh)
#   Intel NPU or GPU    OpenVINO GenAI, and the model exported for it on
#                       this machine; nothing else needs OpenVINO
#
#   setup.sh [--yes] [--no-drivers] [--no-bench]
#
#   --yes         answer yes to every question (sudo is still sudo)
#   --no-drivers  skip system drivers and runtimes (no sudo at all)
#   --no-bench    skip the benchmark
#
# Every step is optional and can be run again later on its own:
# hardware.sh, setup-llamacpp.sh, setup-gpu.sh, setup-npu.sh,
# export-model.sh and `smart-commit bench`. SMART_COMMIT_BIN names the smart-commit binary
# to benchmark; hardware.sh lists the variables tests use.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=/dev/null  # hardware.sh, next to this script
. "$HERE/hardware.sh"
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

# The binary: the one that called us, installed next to share/, on PATH, or
# this checkout's.
if [ -n "${SMART_COMMIT_BIN:-}" ]; then
    SMART_COMMIT=$SMART_COMMIT_BIN
elif [ -x "$HERE/../../../bin/smart-commit" ]; then
    SMART_COMMIT=$HERE/../../../bin/smart-commit
elif command -v smart-commit >/dev/null; then
    SMART_COMMIT=smart-commit
else
    SMART_COMMIT=$HERE/../target/release/smart-commit
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

say "Hardware"
print_inventory

# The plan, from the hardware alone: what each part needs, and why.
say "Plan"
echo "  llama.cpp and the GGUF model (no sudo): the cpu device, and vulkan on the GPUs"
if [ "$(gpu_count)" -gt 0 ]; then
    "$HERE/setup-gpu.sh" --dry-run | grep '^==>' | sed 's/^==> /  /' || true
fi
if has_intel_npu; then
    echo "  Intel NPU: the NPU driver 1.38.0 and Level Zero 1.32.0 (sudo)"
fi
if needs_openvino; then
    echo "  OpenVINO GenAI 2026.3.1 (sudo), for the Intel NPU or GPU (it also gives openvino-cpu),"
    echo "  and the model exported for it here, from Hugging Face"
elif openvino_installed; then
    echo "  the model exported for OpenVINO here, for openvino-cpu (OpenVINO is already installed)"
else
    echo "  not needed: OpenVINO, the Intel NPU driver and Intel's GPU runtime, since"
    echo "  there is no Intel NPU or GPU here"
fi

say "llama.cpp"
"$HERE/setup-llamacpp.sh" || echo "  Could not install llama.cpp; run $HERE/setup-llamacpp.sh again."

if [ "$DRIVERS" = 1 ]; then
    if has_intel_npu; then
        if "$HERE/check-npu.sh" >/dev/null 2>&1; then
            say "Intel NPU: driver and OpenVINO already set up"
        elif ask "Install the Intel NPU driver and OpenVINO (uses sudo)?"; then
            "$HERE/setup-npu.sh"
        fi
    elif has_intel_gpu && ! openvino_installed; then
        if ask "Install OpenVINO, for the Intel GPU (uses sudo)?"; then
            "$HERE/setup-npu.sh" --openvino-only
        fi
    fi
    if [ "$(gpu_count)" -gt 0 ]; then
        say "GPU drivers"
        if ask "Set up the GPUs as planned above (may use sudo)?"; then
            yes_flag=()
            [ "$ASSUME_YES" = 1 ] && yes_flag=(--yes)
            "$HERE/setup-gpu.sh" "${yes_flag[@]}"
        fi
    fi
fi

if needs_openvino || openvino_installed; then
    say "OpenVINO model"
    if "$HERE/export-model.sh" --check; then
        echo "  already exported, every file checked"
    elif ask "Export the model for OpenVINO here (downloads it from Hugging Face; about 10 GB of disk while it runs, a few minutes)?"; then
        "$HERE/export-model.sh" ||
            echo "  Could not export the model; run $HERE/export-model.sh again."
    fi
fi

if [ "$BENCH" = 1 ]; then
    if ! id -nG | grep -qw render && getent group render | grep -qw "$USER"; then
        say "Benchmark postponed"
        echo "  You were just added to the render group, which only applies after"
        echo "  logging out and back in. Then run: smart-commit bench"
    else
        say "Benchmark"
        "$SMART_COMMIT" bench || echo "  Nothing could run the model yet; see above."
    fi
fi

say "Done"
echo "  Generate a message:   git add -p && smart-commit"
echo "  Commit with it:       smart-commit --commit"
echo "  On every git commit:  smart-commit hook install"
echo "  Change the device:    smart-commit device use <npu|intel-gpu|vulkan|cpu|openvino-cpu|auto>"

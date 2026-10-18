#!/usr/bin/env bash
# Installs llama.cpp and its model, which run the `cpu` device on any x86_64
# machine (Intel or AMD) and the `vulkan` device on any GPU with a Vulkan
# driver. Nothing here needs sudo:
#
#   llama.cpp b11381 (the Vulkan build; it also has the CPU code for every
#   x86_64 generation, picked at run time) in $XDG_DATA_HOME/smart-commit/llama.cpp
#   the GGUF model in $XDG_DATA_HOME/smart-commit/models
#
# Every download is checked against a pinned SHA-256.
#
#   scripts/setup-llamacpp.sh [--dry-run] [--server-only]
#
#   --dry-run      print what would be done, change nothing
#   --server-only  only llama.cpp, not the model (`smart-commit model use`
#                  downloads the one chosen)
#
# SMART_COMMIT_LLAMACPP_URL and SMART_COMMIT_GGUF_URL replace the download
# URLs, for tests; the pinned SHA-256 is still required.
set -euo pipefail

LLAMA_BUILD=b11381
LLAMA_URL=${SMART_COMMIT_LLAMACPP_URL:-https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_BUILD/llama-$LLAMA_BUILD-bin-ubuntu-vulkan-x64.tar.gz}
LLAMA_SHA256=6f93bf4138e9ad6a380ff605be1fd5108a66ec1977218eb74613f90634e18c64

GGUF_NAME=qwen2.5-coder-1.5b-instruct-q4_k_m.gguf
GGUF_URL=${SMART_COMMIT_GGUF_URL:-https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/f86cb2c1fa58255f8052cc32aeede1b7482d4361/$GGUF_NAME}
GGUF_SHA256=cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046

DATA=${XDG_DATA_HOME:-$HOME/.local/share}/smart-commit
DRY_RUN=0
MODEL=1
for arg in "$@"; do
    case $arg in
        --dry-run) DRY_RUN=1 ;;
        --server-only) MODEL=0 ;;
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

# run CMD...: runs it, or prints it with --dry-run.
run() {
    if [ "$DRY_RUN" = 1 ]; then
        echo "would run: $*"
    else
        "$@"
    fi
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

say "installing llama.cpp $LLAMA_BUILD in $DATA/llama.cpp"
if [ -x "$DATA/llama.cpp/llama-$LLAMA_BUILD/llama-server" ]; then
    echo "    already installed"
else
    archive=$DATA/llama.cpp/llama-$LLAMA_BUILD.tar.gz
    fetch "$LLAMA_URL" "$LLAMA_SHA256" "$archive"
    run tar -xzf "$archive" -C "$DATA/llama.cpp"
    run rm -f "$archive"
fi

if [ "$MODEL" = 1 ]; then
    say "fetching the GGUF model in $DATA/models"
    model=$DATA/models/$GGUF_NAME
    if [ -f "$model" ] && echo "$GGUF_SHA256  $model" | sha256sum --check --quiet - 2>/dev/null; then
        echo "    already there"
    else
        fetch "$GGUF_URL" "$GGUF_SHA256" "$model"
    fi
    [ "$DRY_RUN" = 1 ] || echo "$GGUF_SHA256  $GGUF_NAME" >"$model.sha256"
fi

if [ "$DRY_RUN" = 0 ]; then
    say "devices seen by llama.cpp (besides the CPU):"
    "$DATA/llama.cpp/llama-$LLAMA_BUILD/llama-server" --list-devices 2>&1 | sed -n '/Available devices/,$p' || true
fi

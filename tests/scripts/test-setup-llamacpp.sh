#!/usr/bin/env bash
# Runs scripts/setup-llamacpp.sh --dry-run, then with llama.cpp already in
# place, then with --server-only: it downloads the pinned build and model
# into the data directory and skips what is there. No network or sudo.
#
#   tests/scripts/test-setup-llamacpp.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT=$ROOT/scripts/setup-llamacpp.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

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

data=$work/data
XDG_DATA_HOME=$data bash "$SCRIPT" --dry-run >"$work/fresh.out" 2>&1
expect fresh "would download: https://github.com/ggml-org/llama.cpp/releases/download/b11381/llama-b11381-bin-ubuntu-vulkan-x64.tar.gz -> $data/smart-commit/llama.cpp/llama-b11381.tar.gz (sha256 6f93bf41"
expect fresh "would run: tar -xzf $data/smart-commit/llama.cpp/llama-b11381.tar.gz"
expect fresh "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf -> $data/smart-commit/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf (sha256 cc324af0"
refute fresh "sudo"
if [ -e "$data" ]; then
    echo "FAIL  fresh: --dry-run wrote $data"
    failures=$((failures + 1))
else
    echo "ok    fresh: nothing written"
fi

# llama.cpp already installed: only the model is left.
mkdir -p "$data/smart-commit/llama.cpp/llama-b11381"
printf '#!/bin/sh\n' >"$data/smart-commit/llama.cpp/llama-b11381/llama-server"
chmod +x "$data/smart-commit/llama.cpp/llama-b11381/llama-server"
XDG_DATA_HOME=$data bash "$SCRIPT" --dry-run >"$work/installed.out" 2>&1
expect installed "already installed"
refute installed "llama-b11381-bin-ubuntu-vulkan-x64.tar.gz"
expect installed "q4_k_m.gguf (sha256"

# --server-only: llama.cpp, not the model.
XDG_DATA_HOME=$work/data2 bash "$SCRIPT" --dry-run --server-only >"$work/server.out" 2>&1
expect server "llama-b11381-bin-ubuntu-vulkan-x64.tar.gz"
refute server "gguf"

bash "$SCRIPT" --frobnicate >"$work/usage.out" 2>&1 && status=0 || status=$?
if [ "$status" = 2 ]; then
    echo "ok    usage: unknown option exits 2"
else
    echo "FAIL  usage: exit $status"
    failures=$((failures + 1))
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all cases passed"

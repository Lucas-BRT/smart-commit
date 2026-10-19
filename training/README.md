# Training pipeline

Exploratory work on the `training` branch; see
[docs/training.md](../docs/training.md) for the goal and the decisions, and
[HYPOTHESES.md](HYPOTHESES.md) for what was tried and measured.

Everything runs on the training machine (RTX 3050 8 GB). Large files live
outside the repository, in `~/.cache/smart-commit-training/`, and in
`data/` and `runs/` here (ignored by git).

## Environment

```bash
mise install python@3.12.15
export UV_PYTHON=$(mise where python@3.12.15)/bin/python3 UV_PYTHON_DOWNLOADS=never
cd training && uv sync --locked
uv run --locked python -I test_build_inputs.py   # checks -U1 trimming against git
```

## Inputs

| What | Pinned by |
|---|---|
| CommitChronicle | revision `5fd076e67b812a9f3d1999e5e40f71715f84bb51`, shards under `~/.cache/smart-commit-training/commit-chronicle/5fd076e/` |
| Qwen2.5-Coder-1.5B-Instruct | revision `2e1fd397ee46e1388853d2af2c993145b0f1098a` (the one `scripts/export-model.sh` pins) |
| llama.cpp | `b11381` (the build smart-commit installs; source tarball sha256 `c07a6aeb3003f40159e336ba548593052288615dabe9f17b1341d512dc278927`) |

## Steps

```bash
C=~/.cache/smart-commit-training
M=$C/models/qwen2.5-coder-1.5b-instruct@2e1fd397ee46e1388853d2af2c993145b0f1098a
SC=../target/release/smart-commit        # cargo build --locked --release

# 1. Commits -> trace inputs + targets (one directory per set of shards).
uv run --locked python -I build_inputs.py data/train12 $C/commit-chronicle/5fd076e/train-000*.parquet
uv run --locked python -I build_inputs.py data/test0 $C/commit-chronicle/5fd076e/test-00000.parquet

# 2. Deterministic selections. eval-v1 comes from the test split, whose
#    repositories never appear in train.
uv run --locked python -I select.py data/test0 data/eval-v1 --conventional 250 --free 250
uv run --locked python -I select.py data/train12 data/h1 --conventional 6000 --free 4000 \
  --max-type-share 0.3 --per-repo 100

# 3. Prompts built by smart-commit, joined with the targets.
uv run --locked python -I make_examples.py data/h1 $SC

# 4. Train (writes runs/h1/adapter and runs/h1/run.json).
uv run --locked python -I train.py data/h1/examples.jsonl $M runs/h1 --quant 16bit

# 5. Merge and quantize like the upstream GGUF.
uv run --locked python -I export_gguf.py $M runs/h1/model-q4_k_m.gguf --adapter runs/h1/adapter \
  --llama-src $C/llama.cpp-src/llama.cpp-b11381 --llama-bin ~/.local/share/smart-commit/llama.cpp/llama-b11381

# 5b. Or keep the upstream GGUF and apply the adapter on it (no re-quantization noise).
uv run --locked python -I export_lora.py $M runs/h1/adapter runs/h1/adapter.gguf \
  --llama-src $C/llama.cpp-src/llama.cpp-b11381
#     then run step 6 with, instead of SMART_COMMIT_GGUF:
#     SMART_COMMIT_LLAMA_SERVER=$PWD/llama-server-lora.sh LORA_GGUF=$PWD/runs/h1/adapter.gguf \
#     LORA_SERVER=~/.local/share/smart-commit/llama.cpp/llama-b11381/llama-server
#     (traces then name the base model only: the adapter is not recorded).

# 6. Evaluate with smart-commit itself, then score.
XDG_CONFIG_HOME=$C/empty-config SMART_COMMIT_GGUF=$PWD/runs/h1/model-q4_k_m.gguf \
  $SC eval data/eval-v1/inputs.jsonl --device vulkan > data/eval-v1/h1.jsonl
uv run --locked python -I score.py data/eval-v1 data/eval-v1/h1.jsonl
```

`data/eval-v1` is frozen: it was selected with the `build_inputs.py` of
commit 35a1ae8, before the version-only filter, which today's `select.py`
applies too; it is identified by its hashes, `inputs.jsonl` sha256 `6d275f0d92c607e5f80cc3be204d5a8a4444391e029abc907eb311b3f29e428c`,
`meta.jsonl` sha256 `9b8d53df9863d6b214c53d2de2839ce18f26fc088d9a3e8a2f84202a44e0faaf`.

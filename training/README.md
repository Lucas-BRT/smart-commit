# Training pipeline

Exploratory work toward smart-commit's own models; see
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
# The learning-curve set of H3 (data/curve; curve-h4 is the same with the
# history paths), and H8's variant without the type cap:
uv run --locked python -I select.py data/train-all data/curve --conventional 24000 --free 16000 \
  --max-type-share 0.3 --per-repo 150 --supported-only
uv run --locked python -I select.py data/train-all data/h8 --conventional 24000 --free 16000 \
  --per-repo 150 --supported-only
# v0.2's extra slices: young repositories (H5) and release bumps whose version
# the diff adds (H7), which the target filters otherwise drop.
uv run --locked python -I select.py data/train-all data/h5 --conventional 1600 --free 2400 \
  --min-history 0 --max-history 9 --per-repo 5 --supported-only
uv run --locked python -I build_inputs.py data/releases $C/commit-chronicle/5fd076e/train-*.parquet --releases
uv run --locked python -I select.py data/releases data/h7 --conventional 400 --free 200 \
  --min-history 0 --per-repo 5 --supported-only --releases
# h3a (H3): the same commits without the targets the label filters reject.
uv run --locked python -I filter_supported.py data/h1 data/h3a

# 3. Prompts built by smart-commit, joined with the targets (`--rules omitted`
#    for a model trained without the fixed instructions, H6).
uv run --locked python -I make_examples.py data/h1 $SC

#    Nested subsets for a learning curve (H3) or a smaller run (H8):
uv run --locked python -I nest.py data/curve/examples.jsonl data/curve/clean 2500 5000 20000 40000

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

`data/eval-v1` is frozen: it was built and selected with the
`build_inputs.py` and `select.py` of commit 35a1ae8, before the version-only
filter, which today's scripts apply; today's `select.py` alone gives other
hashes. It is identified by its hashes, `inputs.jsonl` sha256 `6d275f0d92c607e5f80cc3be204d5a8a4444391e029abc907eb311b3f29e428c`,
`meta.jsonl` sha256 `9b8d53df9863d6b214c53d2de2839ce18f26fc088d9a3e8a2f84202a44e0faaf`.
To rebuild it:

```bash
O=$(mktemp -d)
git show 35a1ae8:training/build_inputs.py > $O/build_inputs.py
git show 35a1ae8:training/select.py > $O/select.py
uv run --locked python -I $O/build_inputs.py data/test0-v1 $C/commit-chronicle/5fd076e/test-00000.parquet
uv run --locked python -I $O/select.py data/test0-v1 data/eval-v1 --conventional 250 --free 250
sha256sum data/eval-v1/*.jsonl
```

`data/eval-short` (young repositories, see H5 in HYPOTHESES.md):
`select.py data/test2to5h data/eval-short --conventional 100 --free 150
--min-history 1 --max-history 5 --per-repo 5`, from `build_inputs.py` over
`test-00002`…`test-00005`; `inputs.jsonl` sha256 starts `42d212c0d2687dad`,
`meta.jsonl` `867c9faf4c5e80bd`.

Bundle `data/kaggle/h8` (H8): `make_examples.py data/h8 $SC`, then
`nest.py data/h8/examples.jsonl data/h8/nocap 20000`, then
`kaggle_bundle.py data/kaggle/h8 --val data/kaggle/b2/val.examples.jsonl
c20k-nocap-0.5b=0.5b:data/h8/nocap-20000.jsonl
c20k-ctl-0.5b=0.5b:data/curve/clean-20000.jsonl`. The examples' sha256
start `6da30e215758959e` and `3d69c19fc6bd0577` (the c20k b2 trained on),
the validation set's `f6350a3c5cb71b58` (b2's).

`data/releases` (H7): `inputs.jsonl` sha256 starts `cbd4239e8433a438`,
`meta.jsonl` `c7e2d480aca635b9`; `data/h5` `9827ecc272759ec9` and
`513484be87384cf5`; `data/h7` `4c6a2cad2087a047` and `a35b23326e894b96`.

`data/val1` (the validation set of b2) is
`select.py data/test1 data/val1 --conventional 250 --free 250 --max-type-share 0.3 --supported-only`
(`meta.jsonl` sha256 starts `2fb1dedba6e20939`; `val1-h4` is the same with
the history paths, as `select.py` writes it today).

## Bundle b3 (v0.2)

About 20k examples with prompts built with `--rules omitted` (H6): 15,600
from the main selection, nested like the curve, plus `data/h5` (H5) and
`data/h7` (H7), and the validation set with the same prompts. Built in both
versions of H8's type cap until H8 decides, each selection in its own
directory under `data/b3/`, since `make_examples.py` writes there:

```bash
py() { uv run --locked python -I "$@"; }
py select.py data/test1 data/b3/val --conventional 250 --free 250 --max-type-share 0.3 --supported-only
py select.py data/train-all data/b3/main-cap30 --conventional 24000 --free 16000 \
  --max-type-share 0.3 --per-repo 150 --supported-only
py select.py data/train-all data/b3/main-nocap --conventional 24000 --free 16000 \
  --per-repo 150 --supported-only
py select.py data/train-all data/b3/h5 --conventional 1600 --free 2400 \
  --min-history 0 --max-history 9 --per-repo 5 --supported-only
py select.py data/releases data/b3/h7 --conventional 400 --free 200 \
  --min-history 0 --per-repo 5 --supported-only --releases
for d in val main-cap30 main-nocap h5 h7; do py make_examples.py data/b3/$d $SC --rules omitted; done
for cap in cap30 nocap; do
  py nest.py data/b3/main-$cap/examples.jsonl data/b3/main-$cap/nested 15600
  cat data/b3/main-$cap/nested-15600.jsonl data/b3/h5/examples.jsonl data/b3/h7/examples.jsonl \
    > data/b3/examples-$cap.jsonl
  py kaggle_bundle.py data/kaggle/b3-$cap --val data/b3/val/examples.jsonl \
    v02-1.5b=1.5b:data/b3/examples-$cap.jsonl v02-0.5b=0.5b:data/b3/examples-$cap.jsonl
done
```

20,019 examples, 56% Conventional; `fix` is 29.0% of them with the cap and
33.1% without. sha256 of the examples: `examples-cap30.jsonl` starts
`d3a10c49147e4605`, `examples-nocap.jsonl` `23b9effc8b202e6d`, the
validation set `71fca918a934d0f4`. The order of the concatenation does not
matter: `train.py` shuffles with a fixed seed.

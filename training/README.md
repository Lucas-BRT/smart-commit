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
uv run --locked python -I test_make_examples.py
uv run --locked python -I test_mine_pairs.py
uv run --locked python -I test_train_dpo.py
uv run --locked python -I test_kaggle_mine_bundle.py
uv run --locked python -I test_kaggle_dpo_bundle.py
uv run --locked python -I test_export_runs.py
cargo build --locked --release -p smart-commit-training   # sc-training: the data tools, in Rust
cargo test --locked -p smart-commit-training              # -U1 trimming against git, scoring, the scenarios
cargo build --locked && cargo test --locked -p smart-commit-training -- --ignored   # every scenario, built twice
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
SCT=../target/release/sc-training        # cargo build --locked --release -p smart-commit-training

# 1. Commits -> trace inputs + targets (one directory per set of shards).
$SCT build-inputs data/train12 $C/commit-chronicle/5fd076e/train-000*.parquet
$SCT build-inputs data/test0 $C/commit-chronicle/5fd076e/test-00000.parquet

# 2. Deterministic selections. eval-v1 comes from the test split, whose
#    repositories never appear in train.
$SCT select data/test0 data/eval-v1 --conventional 250 --free 250
$SCT select data/train12 data/h1 --conventional 6000 --free 4000 \
  --max-type-share 0.3 --per-repo 100
# The learning-curve set of H3 (data/curve; curve-h4 is the same with the
# history paths), and H8's variant without the type cap:
$SCT select data/train-all data/curve --conventional 24000 --free 16000 \
  --max-type-share 0.3 --per-repo 150 --supported-only
$SCT select data/train-all data/h8 --conventional 24000 --free 16000 \
  --per-repo 150 --supported-only
# v0.2's extra slices: young repositories (H5) and release bumps whose version
# the diff adds (H7), which the target filters otherwise drop.
$SCT select data/train-all data/h5 --conventional 1600 --free 2400 \
  --min-history 0 --max-history 9 --per-repo 5 --supported-only
$SCT build-inputs data/releases $C/commit-chronicle/5fd076e/train-*.parquet --releases
$SCT select data/releases data/h7 --conventional 400 --free 200 \
  --min-history 0 --per-repo 5 --supported-only --releases
# h3a (H3): the same commits without the targets the label filters reject.
$SCT filter-supported data/h1 data/h3a

# 3. Prompts built by smart-commit, joined with the targets (`--rules omitted`
#    for a model trained without the fixed instructions, H6). The file list
#    is annotated by default since #10/#11; v0.2 was trained on the plain
#    one: add `--diff plain` to rebuild its examples.
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
$SCT score data/eval-v1 data/eval-v1/h1.jsonl
```

`data/eval-v1` is frozen: it was built and selected with the
`build_inputs.py` and `select.py` of commit 35a1ae8, before the version-only
filter, which today's scripts apply; today's `sc-training select` alone gives other
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
`sc-training select data/test2to5h data/eval-short --conventional 100 --free 150
--min-history 1 --max-history 5 --per-repo 5`, from `sc-training build-inputs` over
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
`sc-training select data/test1 data/val1 --conventional 250 --free 250 --max-type-share 0.3 --supported-only`
(`meta.jsonl` sha256 starts `2fb1dedba6e20939`; `val1-h4` is the same with
the history paths, as `sc-training select` writes it today).

## Preference pairs (DPO)

What SFT never shows the model is what not to write: a history subject copied,
a vague description, an invented detail, a wrong type. The pairs for DPO come
from the model's own samples, judged by the checks of `sc-training score`: a
Conventional header, 72 columns, a history subject copied or nearly, a specific
found nowhere in the diff, a description sharing no word with it and, for
Conventional repositories, a scope the history never used (the prompt asks to
omit it then) and a type other than the real commit's. For each input with a
reply that fails, the rejected one is the reply failing the most serious check
(in that order: the type last); the preferred one passes them all, and by
default is the sample closest in words to the real subject (`--prefer real`
puts the real subject first instead). Every pair carries its repository,
commit, type and history length, to balance the set by.

Mine from inputs the model was not trained on: on its own training data it
fails less, and the pairs would be fewer and easier. The test splits
(`data/test1`, `data/test2to5h`) are not enough for the Conventional
repositories, which are few in them (the first run's 191 Conventional inputs
came from 20 repositories); the second run takes them from `data/train-all`,
without the commits the SFT trained on.
`mine_pairs.py` drops the commits of the evaluation sets' repositories
(`data/eval-v1`, `data/eval-short`), so none of them reaches the pairs.
`sc-training select` does the same before choosing, so the per-repository
caps count what is left, with `--exclude-repos-of DIR...`; and
`--exclude-hashes-of DIR...` leaves out the commits of other selections, such
as `data/curve`, which the SFT trained on: `select` ranks by a hash of the
commit, so without it a small cap per repository picks those very commits.

The first run, on the local GPU, from the test split:

```bash
SCT=../target/release/sc-training
SC=../target/release/smart-commit
LLAMA=~/.local/share/smart-commit/llama.cpp/llama-b11381/llama-server
SRC=data/test2to5h                      # or another split the model did not see
mkdir -p data/dpo/mix
# 1. Inputs: young repositories apart, since the prompt has their short history.
$SCT select $SRC data/dpo/pool  --conventional 6000 --free 4000 --per-repo 10 --supported-only
$SCT select $SRC data/dpo/young --conventional 1600 --free 3000 --per-repo 5 \
  --min-history 0 --max-history 9 --supported-only
cat data/dpo/pool/inputs.jsonl data/dpo/young/inputs.jsonl > data/dpo/mix/inputs.jsonl
cat data/dpo/pool/meta.jsonl   data/dpo/young/meta.jsonl   > data/dpo/mix/meta.jsonl

# 2. Four samples per input from each model, with the prompt it was trained on
#    (v0.1: `--rules full --diff plain`). Stopped, it resumes where it was.
for size in 0.5b 1.5b; do
  python -I mine_pairs.py data/dpo/mix data/dpo/mined-$size \
    ~/.local/share/smart-commit/models/smart-commit-$size-v0.1-q4_k_m.gguf \
    --binary $SC --server $LLAMA --device Vulkan0 --rules full --diff plain --samples 4 --workers 2 \
    --exclude-repos-of data/eval-v1 data/eval-short
done

# 3. The pairs, with how many there are by failure.
for size in 0.5b 1.5b; do
  $SCT pairs data/dpo/mined-$size/candidates.jsonl data/dpo/mined-$size data/dpo/pairs-$size.jsonl
done
```

The second run's selection, mining on Kaggle and pairs follow
[below](#mining-on-kaggle).

With `--prompts-only` (no model, no GPU) `mine_pairs.py` only writes
`prompts.jsonl` beside the inputs it keeps; `--prompts FILE` then samples from
those prompts without the binary, which is how the mining runs on Kaggle.

`mine_pairs.py` is greedy only in its prompts: the replies are sampled at
temperature 0.7 with seeds fixed by the input and the sample, so the same
model, server and inputs give the same candidates. On the Intel GPU of a
laptop, two servers at once, the 0.5B model sampled 24 inputs a minute and the
1.5B 14; on an RTX 3050 (Vulkan), also two at once, about 150 and 70. Both
sizes can run at once, or one after the other with `--device` set to the GPU's
name in `llama-server --list-devices`. The servers run with `--cache-ram 0`:
every prompt is new, and the default prompt cache (8 GiB each) filled a
24 GiB machine.

The selection above leaves 5,688 inputs, 3,551 after the evaluation sets'
repositories are dropped, of which 191 Conventional. From them the first
judge (no scope or type checks, the real subject preferred) made, for the
0.5B, 1,287 pairs (964 with the real subject chosen; rejected replies by
failure: ungrounded 619, copied 271, unsupported 258, long 134, format 5) and,
for the 1.5B, 1,255 (916; 597, 275, 239, 139, 5), 72 and 76 of them
Conventional. The judge has changed since: the same candidates now give 1,343
and 1,314 pairs, 128 of the 0.5B's Conventional.

The second run, from 5,500 inputs (4,500 Conventional of 680 repositories and
1,000 of young repositories) with six samples each: 4,052 pairs for the 0.5B
(3,363 Conventional, 689 young; rejected replies by failure: type 1,295, scope
598, ungrounded 902, copied 479, unsupported 374, long 226, format 178) and
3,926 for the 1.5B. `balance` turned the 0.5B's, with the first run's free
ones, into 3,615 pairs of training in 826 repositories and 514 of validation,
or 3,172 with `--match-marginals`.

### Mining on Kaggle

How the quota is spent, what a session costs and what to run in it:
[docs/training.md](../docs/training.md#kaggles-quota).

A notebook runs as a kernel: `kaggle/mine-kernel-metadata.json` and
`kaggle/dpo-kernel-metadata.json` are the settings of the two (the image by
digest, a T4 x2, the internet, private); `dataset_sources` is the dataset of
the bundle of the run, and is changed to it before pushing:

```bash
K=$(mktemp -d)
cp kaggle_dpo.ipynb $K/smart-commit-dpo.ipynb
sed 's#training-dpo2-sweep2#training-NAME-OF-THE-BUNDLE#' kaggle/dpo-kernel-metadata.json > $K/kernel-metadata.json
kaggle kernels push -p $K        # then: kaggle kernels status lucasbrt/smart-commit-dpo
```

The same mining on two T4s, when the local GPU is busy: the prompts are built
here, without a GPU, and only the sampling runs there. `kaggle_mine_bundle.py`
splits the inputs into shards (every Nth input to each), pins what the
notebook downloads (llama.cpp's CUDA 12.8 build by SHA-256, the models by the
revision and SHA-256 of `src/models/registry.rs`) and orders the jobs, a model
and a shard each, the longest first.

The inputs of the second run are mostly Conventional (the first had 7%), from
`data/train-all` without the commits the SFT trained on (`data/curve`) and
without the evaluation sets' repositories, at most 8 commits per repository
and 22% of a type (young repositories apart):

```bash
EX="--exclude-hashes-of data/curve --exclude-repos-of data/eval-v1 data/eval-short data/val1"
$SCT select data/train-all data/dpo2/conv  --conventional 4500 --free 0 --per-repo 8 \
  --max-type-share 0.22 --supported-only $EX
$SCT select data/train-all data/dpo2/young --conventional 1000 --free 0 --per-repo 3 \
  --min-history 0 --max-history 9 --max-type-share 0.3 --supported-only $EX
mkdir -p data/dpo2/mix
cat data/dpo2/conv/inputs.jsonl data/dpo2/young/inputs.jsonl > data/dpo2/mix/inputs.jsonl
cat data/dpo2/conv/meta.jsonl   data/dpo2/young/meta.jsonl   > data/dpo2/mix/meta.jsonl
python -I mine_pairs.py data/dpo2/mix data/dpo2/mine --binary $SC --prompts-only \
  --rules full --diff plain --exclude-repos-of data/eval-v1 data/eval-short data/val1
python -I kaggle_mine_bundle.py data/kaggle/dpo2-mine data/dpo2/mine --samples 6
# upload it as a private dataset, run kaggle_mine.ipynb on it with GPU T4 x2 and internet,
# download the <model>-shard-K.tar.gz archives and unpack them in data/dpo2/mined/.
# A shard's candidates line up with that shard's inputs, so the pairs are made per
# shard, with the shard's directory of the bundle, and concatenated:
for size in 0.5b 1.5b; do
  for k in 0 1 2 3; do
    $SCT pairs data/dpo2/mined/$size-shard-$k/candidates.jsonl data/kaggle/dpo2-mine/shard-$k \
      data/dpo2/pairs-$size-$k.jsonl
  done
  cat data/dpo2/pairs-$size-?.jsonl > data/dpo2/pairs-$size.jsonl
done
```

The notebook checks each download against its SHA-256, samples four inputs of
each model first (so a CUDA build that does not fit the image fails in a
minute, not after an hour), and packs a job as soon as it ends.

### Balancing the pairs

`sc-training balance` turns the pairs of the models' shards into the training
and validation sets. The validation set is every pair of about a tenth of the
repositories (chosen by a hash of the repository, so it does not depend on the
order of the files); the training set takes at most `--max-per-repo` pairs per
repository, at most `--max-type-share` of a bucket in one real type, and at
most a quota of pairs per bucket (`--conventional`, `--young` for repositories
with fewer than ten commits of history, `--free`), in the order of a hash of
the commit. Repeated pairs are dropped. It prints how the training set came
out: pairs by bucket, type and failure, repositories, and how even the lengths
are (the chosen reply longer in about half the pairs, as in the first run).
`--max-length-ratio` drops pairs of very different length but is off, since a
ratio of 1.6 left the chosen replies 4.6 columns shorter than the rejected.
`--match-marginals` drops pairs until the chosen and the rejected replies have
the same distribution of type and the same share with a scope (within
`--marginal-tolerance`, 0.02 of imbalance per pair). The rejected reply is a
sample of the model, which errs most in what it writes most, so unmatched
pairs add up to "fewer `fix` and `feat`, more `chore`, no scope", and a run on
them did that: with the marginals unequal the loss is lowered most cheaply by
moving the model's habit, not by learning from the diff. On the second run's
pairs it drops 443 of 3,615 and the imbalance falls from 820 to 107.

```bash
$SCT balance data/dpo2/pairs-0.5b.jsonl --out data/dpo2/balanced-0.5b \
  --max-per-repo 12 --max-type-share 0.3 --conventional 2400 --young 400 --free 1200
```

### Training on the pairs

`train_dpo.py` starts from a fine-tune: it merges the SFT adapter into the
base model (as `export_gguf.py` does) and trains a new LoRA on top with the
sigmoid DPO loss, `-log σ(β · ((πc − πr) − (refc − refr)))` over the sum of
the log-probabilities of each reply. With the new adapter disabled the model is
the SFT one, so that is also the reference, and only one copy is loaded. The
loss starts at ln 2 (0.693); `accuracy` is the share of pairs whose chosen
reply gained more than the rejected one; `chosen_reward` and `rejected_reward`
are β times how much more likely the model made each than the reference did
(the chosen one falling is the sign of the model forgetting how to write it).
`--val` takes the validation set of `balance`, scored every `--eval-every`
steps (`val_loss`, `val_accuracy`, ...), and `--sft-weight` adds the chosen
reply's per-token loss to the DPO loss. Defaults: β 0.1, learning rate 2e-5,
rank 16, 2 pairs per pass and 8 passes per step, one epoch.

```bash
M=$C/models/qwen2.5-coder-0.5b-instruct@ea3f2471cf1b1f0db85067f1ef93848e38e88c25
uv run --locked python -I train_dpo.py data/dpo/pairs-0.5b.jsonl $M \
  runs/kaggle-v2/c40k-0.5b/adapter runs/dpo-0.5b --merged runs/dpo-0.5b/merged
uv run --locked python -I export_gguf.py runs/dpo-0.5b/merged runs/dpo-0.5b/model-q4_k_m.gguf \
  --llama-src $C/llama.cpp-src/llama.cpp-b11381 --llama-bin ~/.local/share/smart-commit/llama.cpp/llama-b11381
```

The merged model is the `BASE_DIR` of step 5, without `--adapter`. Then step 6
as for any model.

On Kaggle (a T4 has no bf16, so `--dtype fp16`) `kaggle_dpo_bundle.py` packs
the runs, each with its adapter's two files, its pairs and its validation
pairs, and `kaggle_dpo.ipynb` runs them, one per GPU, packing each as soon as
it ends. A run is `NAME=MODEL:SFT_ADAPTER:TRAIN:VAL[:ARGS]`:

```bash
python -I kaggle_dpo_bundle.py data/kaggle/dpo2-train \
  "b01-0.5b=0.5b:runs/kaggle-v2/c40k-0.5b/adapter:data/dpo2/balanced-0.5b/train.jsonl:data/dpo2/balanced-0.5b/val.jsonl:--beta 0.1" \
  "b03-0.5b=0.5b:runs/kaggle-v2/c40k-0.5b/adapter:data/dpo2/balanced-0.5b/train.jsonl:data/dpo2/balanced-0.5b/val.jsonl:--beta 0.3 --sft-weight 0.1"
# (`--per-gpu 3` before the runs starts three at once on each GPU: a 0.5B run uses
# under 2 GiB of a T4's 15; the 1.5B needs about 4–5, so two)
# upload it, run kaggle_dpo.ipynb on it, and unpack the archives in runs/kaggle-dpo2/;
# the adapter is trained on the merged SFT model, so both are merged, in that order:
uv run --locked python -I export_gguf.py $M runs/kaggle-dpo2/b01-0.5b/model-q4_k_m.gguf \
  --adapter runs/kaggle-v2/c40k-0.5b/adapter --adapter runs/kaggle-dpo2/b01-0.5b/adapter \
  --llama-src $C/llama.cpp-src/llama.cpp-b11381 --llama-bin ~/.local/share/smart-commit/llama.cpp/llama-b11381
```

Merging the two adapters one after the other gives the very weights `--merged`
wrote (checked: the largest difference is 0.0).

To choose among runs, `export_runs.py` exports every run of a directory (the SFT
adapter, then the run's, one GGUF each, kept) and prints `NAME=GGUF` for them,
and `sc-training compare` evaluates each model on the
validation set (`data/val1`: its repositories are in none of the pairs) with
`smart-commit eval`, keeps the traces, and prints the metrics of each and, for
each, the cases it gains and loses against the baseline, net of the direction
of the metric, with `*` where McNemar's p is below 0.05. It also shows the
share of replies with a scope (`scoped`) and how many of each type each model
writes next to the real commits', and the share of content words in none of the
history, the paths and the diff (`novel`, the guard against invention, and a
fall to a third of the real commits' says the model repeats the diff instead
of saying what changed): a model that stops writing scopes has a `scope_known`
of 1.0, and one that writes `chore` for everything a `first_try` of 1.0, and
only these show it. `--read 30` prints that many cases side by side (half
Conventional, the same whatever the models): what no metric counts is seen
there. A candidate costs
about two minutes of GPU on 500 inputs. The hand-written scenarios
(`sc-training scenarios build data/scenarios-eval $SC --split eval`, 43 cases,
14 Conventional) are a second set for the same purpose: small, so they confirm
more than they decide, and they add the column `expected`, the share of cases
whose message matches the patterns written for them. The evaluation sets
`eval-v1` and `eval-short` are for the model chosen, once:

```bash
$SCT compare runs/compare $SC --sets data/val1 data/scenarios-eval --read 30 \
  --baseline sft=runs/kaggle-v2/c40k-0.5b/model-q4_k_m.gguf \
  $(uv run --locked python -I export_runs.py $M runs/kaggle-v2/c40k-0.5b/adapter runs/kaggle-dpo2 \
      --llama-src $C/llama.cpp-src/llama.cpp-b11381 --llama-bin ~/.local/share/smart-commit/llama.cpp/llama-b11381)
```

## Scenarios

Hand-written cases from `scenarios/`, in the same layout as a selection:
`sc-training scenarios build data/scenarios-eval $SC --split eval` (or
`--split train`) writes `inputs.jsonl` and `meta.jsonl`, which step 3
(`make_examples.py`) and step 6 (`smart-commit eval`, `sc-training score`) take
unchanged. How to write a case: [scenarios/README.md](scenarios/README.md).

## Bundle b3 (v0.2)

About 20k examples with prompts built with `--rules omitted` (H6): 15,600
from the main selection, nested like the curve, plus `data/h5` (H5) and
`data/h7` (H7), and the validation set with the same prompts. Built in both
versions of H8's type cap until H8 decides, each selection in its own
directory under `data/b3/`, since `make_examples.py` writes there:

```bash
py() { uv run --locked python -I "$@"; }
SCT=../target/release/sc-training
$SCT select data/test1 data/b3/val --conventional 250 --free 250 --max-type-share 0.3 --supported-only
$SCT select data/train-all data/b3/main-cap30 --conventional 24000 --free 16000 \
  --max-type-share 0.3 --per-repo 150 --supported-only
$SCT select data/train-all data/b3/main-nocap --conventional 24000 --free 16000 \
  --per-repo 150 --supported-only
$SCT select data/train-all data/b3/h5 --conventional 1600 --free 2400 \
  --min-history 0 --max-history 9 --per-repo 5 --supported-only
$SCT select data/releases data/b3/h7 --conventional 400 --free 200 \
  --min-history 0 --per-repo 5 --supported-only --releases
for d in val main-cap30 main-nocap h5 h7; do py make_examples.py data/b3/$d $SC --rules omitted --diff plain; done
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

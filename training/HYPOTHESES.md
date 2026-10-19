# Hypotheses and measurements

Every number comes from `score.py` on `data/eval-v1` (500 commits from 70
repositories of CommitChronicle's test split: 250 Conventional from 7
mostly-Conventional repositories, 250 from 63 others), generated with
`smart-commit eval --device vulkan` on the RTX 3050. With n = 250, a rate
near 0.5 has a standard error of about 0.03: differences under ~0.06 are
noise.

`type_match` and `scope_known` are measured on the conventional set;
`copied` and `word_f1` on each set (conv / free).

## Results

| Run | type_match | scope_known | first_try | fallback | copied conv / free | word_f1 conv / free |
|---|---|---|---|---|---|---|
| base 1.5B, upstream Q4_K_M | 0.480 | 0.568 | 0.984 | 0.012 | 0.064 / 0.136 | 0.254 / 0.153 |
| base 1.5B, our Q4_K_M (control) | 0.416 | 0.544 | 0.996 | 0.000 | 0.068 / 0.112 | 0.246 / 0.138 |
| base 0.5B, upstream Q4_K_M | 0.260 | 0.788 | 0.756 | 0.072 | 0.372 / 0.156 | 0.182 / 0.113 |
| base 0.5B, our Q4_K_M (control) | 0.332 | 0.888 | 0.864 | 0.068 | 0.468 / 0.188 | 0.191 / 0.121 |
| H1 (1.5B), LoRA on upstream GGUF | **0.660** | 0.860 | 1.000 | 0.000 | 0.040 / 0.076 | **0.365 / 0.254** |
| H1 (1.5B), merged, our Q4_K_M | 0.652 | 0.840 | 1.000 | 0.000 | 0.032 / 0.032 | 0.352 / 0.238 |
| H2 (0.5B), LoRA on upstream GGUF | 0.604 | 0.848 | 1.000 | 0.000 | 0.064 / 0.068 | 0.333 / 0.237 |
| H2 (0.5B), merged, our Q4_K_M | 0.624 | 0.856 | 1.000 | 0.000 | 0.044 / 0.064 | 0.329 / 0.242 |

Speed on the CPU (Ryzen 5 5500, 6 cores, `--device cpu` as shipped, the
first 50 commits of eval-v1, model load counted once):

| Model | s/message | attempts | chars/reply |
|---|---|---|---|
| base 1.5B | 2.88 | 52 | 72 |
| H1 merged | 2.61 | 50 | 46 |
| base 0.5B | 1.78 | 62 | 78 |
| H2 merged | **1.26** | 50 | 42 |

Reference: answering `fix` to every conventional commit gives a
type_match of 0.41 (102 of 250).

## Base model behaviour

- 1.5B: `fix` for 86.8% of conventional commits (real share: 41%).
- 0.5B: `feat` for most commits, copies a history subject verbatim 37% of
  the time on the conventional set, and fails the convention on the first
  try 24% of the time.

## Hypotheses

### H0. Our quantization is not worse than the upstream file — partly false

Our Q4_K_M (llama.cpp b11381, from the bf16 weights) is 986 MB against the
upstream 1117 MB, and its type_match is 0.064 lower: within two standard
errors, but every fine-tune must be compared with the control, not with
the upstream file. The layouts differ: upstream stores `output.weight`
separately (Q6_K) with `token_embd` in Q4_K, ours keeps the tied embedding
as one Q6_K tensor (the model's `tie_word_embeddings`), hence 130 MB less.
The output layer has the same precision either way, so the gap is
quantization noise in the other layers. It moves the 0.5B too (0.26 →
0.33 type_match, the other way), by more than the sampling noise.

Consequence: fine-tunes are judged twice, merged and re-quantized against
our control, and as a LoRA applied on the upstream GGUF
(`export_lora.py` + `llama-server-lora.sh`) against the upstream scores.
The second isolates the fine-tune; the first is what a published merged
model would be.

### H1. A LoRA on 10k examples fixes the type bias — confirmed

- Data: `data/h1`, 6000 Conventional commits from 131 repositories (at most
  30% of any type: fix 1800, feat 1473, chore 1037, refactor 581, docs
  470, test 354, …) and 4000 from 967 other repositories; at most 100 per
  repository; history of at least 10 subjects.
- Training: LoRA r=16, alpha=32, dropout 0.05, all linear layers, lr 2e-4
  cosine, warmup 3%, batch 4 × 4, 1 epoch, seed 0, loss on the reply only.
- Where: the first local run (bf16 base on the 3050: 0.69 s/example and
  4.8 GiB peak, against 0.85 s/example and 4.1 GiB for QLoRA) stalled when
  the machine suspended, so H1 runs on Kaggle (`kaggle_train.ipynb`, bundle
  `lucasbrt/smart-commit-training-b1`, T4 without bf16): QLoRA, 4-bit NF4
  base with fp16 compute.
- Expectation: type_match well above the control's 0.416, copied down,
  word_f1 up; first_try must not drop.
- **Result.** type_match 0.48 → 0.66 against upstream (0.416 → 0.652
  merged, against the control), scope_known 0.57 → 0.86 (no more file
  paths as scopes), word_f1 +0.11 on both sets, first_try 1.000, copying
  halved. The generated types now follow the real distribution (fix 104
  vs 102 real, feat 55 vs 52, docs 32 vs 31, chore 19 vs 27). Merged and adapter agree
  within noise, so the merged model is publishable as is. Kaggle T4, 76 min,
  train loss 2.46 (3.19 at the start, still falling at the end).
- **New failure modes, seen by reading samples**: vaguer subjects ("fix
  type error", "fix typo") and invented specifics: `chore(release): 1.1.0`
  for a real `chore(publish): 5.5.0-beta.6`, a version the diff may not
  show. word_f1 cannot catch either; a faithfulness check is needed (the
  LLM judge of docs/training.md, or a deterministic one: numbers and
  identifiers in the subject must appear in the diff or the history).

### H2. The 0.5B, trained the same way, gets close to the trained 1.5B — confirmed

Same data and recipe as H1, on Kaggle's second T4. The 0.5B reads the
prompt about 1.6× faster on the CPU (prefill 1916 against 2998 ms in
docs/training.md); untrained it was not faster end to end only because it
copied history subjects and ran to the 128-token limit. Load time barely
differs (311 against 369 ms). If it gets within noise of H1 on type_match
and word_f1, with first_try ≥ 0.98, it is the better default for slow
CPUs; it still has no OpenVINO export for the NPU and Intel GPU.

**Result.** type_match 0.60–0.62, within ~0.04 of H1 (about one standard
error), and better than the untrained 1.5B on every metric; first_try
went from 0.76 to 1.000 and copying from 0.37 to 0.06. On the CPU it is
2.3× faster than the shipped 1.5B (1.26 against 2.88 s/message), from a
shorter prompt read and replies of 42 characters instead of 78. Kaggle T4,
35 min, train loss 2.71.

### H4. Showing where each scope is used fixes scopes copied from the history — planned

Found in real use, on this repository: for a staged `mise.toml` (Rust
toolchain bump), H2 wrote `feat(training): rust 1.99`, taking the scope the
recent history uses most; H1 wrote `chore: update rust version in
mise.toml`. The prompt lists the scopes used here but not which files each
one covers, so a small model picks the most frequent one. `scope_known`
even rewards that: `training` is a known scope, just the wrong one.

- **Prompt (Rust).** `Scopes used here: training (training/,
  docs/training.md), cli (src/main.rs)`: for each scope, the paths its
  commits touched most, from `git log --name-only` over the same
  `style.history_depth` commits, deterministic and cheap. A change touching
  none of them should get no scope, as the rule already says. It changes
  the prompt: snapshots, the trace fixtures regenerated with
  `tests/fixtures/make-traces.sh`, `trace::Inputs` extended (old traces
  must still read), and eval-v1 before and after on the base models.
- **Training data.** CommitChronicle has the files of every commit, so the
  same map can be built for the history of each example.
- **Metric.** `scope_fits`: the generated scope was used before by commits
  touching the same areas (top-level directory or file) as the staged
  ones, or there is no scope and none fits. Report it for the base models,
  H1/H2 and the real subjects.
- **Expectation.** scope_fits up on the conventional set, type_match and
  word_f1 not worse, at a cost of a few dozen prompt tokens.
- **Baseline of the metric** (eval-v1h: eval-v1 plus the paths of each
  history commit, from the same shard): the real subjects fit 0.752; base
  1.5B 0.464, base 0.5B 0.564; H1 0.708, H2 0.728. The fine-tunes already
  got most of the way on CommitChronicle; the real-use case may be typical
  of repositories with one dominant recent scope.
- **The prompt alone does nothing** (implemented: `feat(prompt): show where
  each scope's commits went`). scope_fits: base 1.5B 0.464 → 0.484, base
  0.5B 0.564 → 0.564, H1 0.708 → 0.688, H2 0.728 → 0.688 (trained on the
  old prompt); all within noise, and H2 still writes `feat(training)` for
  the mise.toml case. It needs training with the new prompt: H4b.
- **Weakness seen on this repository:** docs/ is among the places of almost
  every scope (each feature updates the docs here), so the map is long
  (~100 tokens for 8 scopes) and tells scopes apart less. Possible fix:
  leave out places shared by most scopes, the way idf discounts common
  words.

### H-split. The model can tell when the staged changes are several commits — planned

To be tried on a branch of its own (`training-split`, from `training`) once
H1/H2 show the basic fine-tune works: it changes the reply format and the
Rust side.

- **Data.** CommitChronicle has no "should have been split" label: mixed
  commits carry one message. Synthesize positives by joining 2–3 real
  commits of the same repository, close in time and with **no file in
  common**, into one input; the target is the split reply with their real
  subjects. Pairs chosen by hash, deterministic like the rest.
- **Negatives.** Ordinary commits (most of the data), and hard negatives:
  large single-purpose commits (refactors across many files, renames), so
  "many files" does not become "split".
- **Reply format.** Files grouped, validated by the program like the
  sanitizer validates messages: the groups must partition the staged files
  exactly, every path must exist, every subject must pass the sanitizer;
  otherwise the suggestion is dropped and the normal message is used. A
  GBNF grammar can enforce the shape.

  ```
  !split
  feat(diff): parse renamed files | src/diff.rs src/git.rs
  docs: describe rename detection | docs/usage.md
  ```

  Alternative in two steps: one call only groups the files, then the
  current pipeline runs per group on that group's diff. More calls, but it
  reuses this prompt and model, and each message sees its whole diff
  instead of sharing the 1024 tokens.
- **Product.** The hook never blocks: it writes the message for the whole
  change plus comment lines git drops (`# smart-commit: looks like 2
  commits: src/diff.rs src/git.rs | docs/usage.md`). The interactive review
  gets a "split" action that unstages, stages and commits each group. Split
  by file first; hunks within one file (partial patches) later.
- **Metrics.** Precision and recall of the detection on a held-out
  synthetic set (unseen repositories), precision first: a false alarm
  costs more than a miss. A small set of real mixed commits labelled by
  hand, since synthetic ones are too obvious. eval-v1 must not get worse.
- **Baseline to beat.** A deterministic heuristic without the model
  (files in unrelated areas, code with docs and lockfiles), for the hook's
  hint only.

## Ideas not tried yet

- Train without the rules in the system prompt (shorter prefill); needs a
  `PromptOptions` switch in smart-commit.
- More data / a second epoch: the loss was still falling at the end of
  H1 and H2. A learning curve (2.5k, 5k, 10k, 20k, 40k examples) finds the
  plateau; it needs the rest of CommitChronicle and a validation loss in
  train.py.
- A faithfulness metric (see H1's failure modes) before optimising further.
- A body in the target when the real commit has one.

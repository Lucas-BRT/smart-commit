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

### H3. More data keeps helping, and cleaner labels help more — partly true

Bundle b2 (`lucasbrt/smart-commit-training-b2`, uploaded before the scope
map of H4, so trained on the prompt without it): a learning curve on the
0.5B with nested subsets (`nest.py`) of 2.5k, 5k, 20k and 40k examples,
the 40k set on the 1.5B, and h3a, the 0.5B on 9,278 examples: H1's set
with the unsupported and version-only targets dropped and nothing chosen
in their place, `filter_supported.py data/h1 data/h3a` (first made with an
unrecorded inline script; the versioned one rebuilds `inputs.jsonl`,
`meta.jsonl` and the bundle's `examples.jsonl`, sha256 `27747f54…`, byte for
byte). Same recipe as H1/H2 otherwise.

Scored with smart-commit at `db389c2` (Vulkan, Intel Arc iGPU), merged
and re-quantized with `export_gguf.py`, against the controls of H0
re-run with the same binary. That binary has the copy guard of H5, which
rejects a first reply repeating a history subject: first_try is lower and
copied is 0 for every model, so these numbers are comparable with each
other only, not with the Results table above.

eval-v1, conventional set (250), plus first_try and word_f1 on the free
set (250):

| Model | type_match | scope_known | first_try | fallback | word_f1 | ungrounded | free first_try | free word_f1 |
|---|---|---|---|---|---|---|---|---|
| 1.5B upstream | 0.456 | 0.580 | 0.908 | 0.068 | 0.245 | 0.180 | 0.840 | 0.143 |
| 1.5B control | 0.408 | 0.516 | 0.928 | 0.004 | 0.245 | 0.172 | 0.868 | 0.139 |
| c40k-1.5b | **0.644** | 0.852 | 0.948 | 0.016 | **0.362** | 0.084 | 0.908 | **0.257** |
| 0.5B upstream | 0.168 | 0.756 | 0.392 | 0.400 | 0.133 | 0.368 | 0.756 | 0.101 |
| 0.5B control | 0.236 | 0.880 | 0.424 | 0.512 | 0.153 | 0.296 | 0.720 | 0.098 |
| c2500-0.5b | 0.516 | 0.852 | 0.932 | 0.056 | 0.296 | 0.084 | 0.924 | 0.217 |
| c5k-0.5b | 0.552 | 0.864 | 0.904 | 0.064 | 0.301 | 0.096 | 0.864 | 0.214 |
| h3a-0.5b | 0.564 | 0.856 | 0.904 | 0.060 | 0.308 | **0.068** | 0.916 | 0.228 |
| c20k-0.5b | 0.632 | 0.828 | 0.936 | 0.036 | 0.323 | 0.080 | 0.924 | 0.231 |
| c40k-0.5b | 0.640 | 0.856 | **0.980** | **0.012** | 0.342 | 0.108 | **0.928** | 0.233 |

eval-short (197, young repositories):

| Model | first_copied | fallback | ungrounded | word_f1 |
|---|---|---|---|---|
| 1.5B upstream | 0.147 | 0.102 | 0.142 | 0.167 |
| 1.5B control | 0.152 | 0.051 | 0.127 | 0.160 |
| c40k-1.5b | 0.112 | **0.030** | **0.107** | **0.295** |
| 0.5B upstream | 0.371 | 0.345 | 0.320 | 0.125 |
| 0.5B control | 0.365 | 0.386 | 0.315 | 0.125 |
| c2500-0.5b | 0.096 | 0.081 | 0.127 | 0.214 |
| c5k-0.5b | 0.157 | 0.142 | 0.137 | 0.232 |
| h3a-0.5b | 0.112 | 0.086 | 0.137 | 0.242 |
| c20k-0.5b | **0.081** | 0.081 | 0.117 | 0.237 |
| c40k-0.5b | 0.137 | 0.091 | 0.137 | 0.246 |

Training on Kaggle T4: 14, 22, 37, 75 and 147 min for c2500, c5k, h3a,
c20k and c40k on the 0.5B (train loss 2.81, 2.73, 2.74, 2.63, 2.58), 394
min for c40k on the 1.5B (2.35). Validation loss still fell from 20k to
40k (2.84 → 2.78 on the 0.5B).

- **The curve flattens at about 20k for the type.** type_match 0.516,
  0.552, 0.632, 0.640 (standard error about 0.03): 20k → 40k is within
  noise. 40k still buys first_try (0.936 → 0.980), fallback (0.036 →
  0.012) and a little word_f1 (0.323 → 0.342).
- **The 0.5B trained on 40k matches the 1.5B trained on 40k** on type and
  scope (0.640 against 0.644); the 1.5B keeps a lead on word_f1 and on
  young repositories (fallback 0.030 against 0.091).
- **4× the data did not move the 1.5B**: H1 (10k) had 0.652 merged, c40k
  0.644, though with an older binary.
- **No amount of this data fixes young repositories**: the 0.5B falls
  back on 8–14% of eval-short whatever the size. That is H5's case: the
  sets have no short histories, more of them does not help.
- **Filtered labels (h3a)** sit where ~10–15k unfiltered examples would,
  with the lowest ungrounded (0.068); a small effect, not enough to
  replace volume.

Next: H5 and H6 in one bundle on 20k examples, plus short-history ones,
rather than more of the same data.

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

### H5. Training on young repositories stops the copying — planned

Found in real use: a new repository with one commit (`feat: Add a simple
hello world program`) and a one-line change adding a `println!` with a sum.
Every model tried, base and fine-tuned, 0.5B and 1.5B, answered with the
history subject verbatim; H1 came closest (`... with a print statement`).

Cause in our data: `select.py` requires `--min-history 10`, for training
and for eval-v1, so neither has ever seen a young repository. With one
example and "follow their style", repeating it is the easy answer.

- **Guard, done on main** (`fix(generate): retry when the reply repeats a
  history subject`): a reply repeating a history subject is rejected and
  retried with a note naming it; repeated again, it falls back to the file
  list. Nothing copied reaches the user, but the fallback is generic.
- **eval-short**: young repositories, 1–5 history subjects, from test
  shards 2–5 (`select.py --min-history 1 --max-history 5 --per-repo 5`):
  47 Conventional commits from 13 repositories, 150 others from 132.
  Too few Conventional commits for type_match; it measures copying.

  | Model | first reply copied | fallback | ungrounded | word_f1 |
  |---|---|---|---|---|
  | base 1.5B | 0.142 | 0.102 | 0.142 | 0.170 |
  | base 0.5B | 0.365 | 0.350 | 0.325 | 0.122 |
  | H1 | 0.041 | 0.010 | 0.142 | 0.288 |
  | H2 | 0.102 | 0.071 | 0.107 | 0.222 |

  The fine-tunes copy far less than the base models, but H2 still falls
  back on 7% of young repositories, against none on eval-v1.
- **Data.** Add examples with 0–9 history subjects (`--min-history 0`),
  about a fifth of the set, so the model learns to describe the diff when
  the history says little. Watch that eval-v1 does not get worse.
- **Prompt, if data is not enough.** With a very short history, present
  the examples as a format reference only ("follow their format, do not
  repeat them").
- **Expectation.** first reply copied and fallback on eval-short down to
  H1's level for the 0.5B, eval-v1 unchanged.

### H6. The rules can live in the weights instead of the prompt — planned

Context distillation: train the model on prompts without the fixed rules
(reply with the message only, 72 columns, the imperative, the Conventional
Commits format), keeping only what varies per repository (types, scopes,
examples, language, ticket) and the diff. The behaviour the rules asked
for moves into the weights; the prompt shrinks.

- **Why not soft prompts.** Learning a few embedding vectors to replace the
  rules (prompt or prefix tuning) would save the same tokens, but neither
  llama-server nor OpenVINO GenAI accept embeddings in place of text, so it
  needs runtime changes on every backend. Context distillation needs none.
- **Gain.** The fixed rules are about 200–250 tokens, the median prompt
  about 400 with the chat template (2000 prompts of train shard 0: p50 406,
  p90 816 tokens).
  Prefill scales about linearly with prompt tokens and is about 90% of the
  time per message on the CPU, so typical prompts would read 40–60% faster.
  Measure it, do not trust this estimate.
- **Rust.** A `PromptOptions` switch (e.g. `prompt.rules = full | none`)
  that a trained model turns on; base models keep the full rules. It must
  reach traces through `PromptOptions`, like every setting that changes
  what is generated. Which models need `none` could be recorded in
  `models::REGISTRY`, so the user does not set it by hand.
- **Training.** The same pipeline, with `make_examples.py` building prompts
  with the switch on. Best trained together with H5 (young repositories) in
  one bundle, plus more data if the b2 learning curve says so.
- **Metrics.** eval-v1 and eval-short with the switch on against H2/H1 with
  the full prompt: first_try and fallback must not move (they are what the
  rules were for), type_match and word_f1 within noise; CPU seconds per
  message on the same 50 commits as before.
- **Risk.** A model trained without the rules depends on the training for
  the format; an untrained model with the switch on would fall apart, so
  the switch must follow the model, never be a free setting.

### H7. The lists in the prompt are read as closed because the data rarely leaves them — planned

Found in real use with v0.1 (smart-commit 0.5.0, Vulkan), each with a
trace fixture:

- `mise.toml` added at the root of this repository, whose recent subjects
  are all scoped and never `chore` ("Types used here: docs, feat, fix,
  test"). 0.5B: `feat(training): add rust 1.99 to mise.toml`
  (`conventional-en-closed-types`); 1.5B: `docs(mise): add rust to tools`
  (`conventional-en-file-scope`). The base 0.5B answered `chore: add
  mise.toml`.
- The 0.5.0 version bump: `feat(model): update smart-commit to 0.5b-v0.1`
  (`conventional-en-release`), with `chore` in the list: the version is
  taken from the last subject's model name.
- Fixtures added one after the other: the 0.5B repeated the previous
  commit's subject, then another history subject after the retry named
  the first, and the message came from the file list
  (`conventional-en-copied-twice`).

Cause in our data, measured on `data/curve/clean-40000.jsonl` (c40k, the
v0.1 set), on the 25,652 targets matching `type(scope)!: `:

- The target's type is in "Types used here" for 87.7%: the model learned
  to choose inside the list, and `chore` or `build` are out of reach when
  the recent history lacks them.
- When every history subject has a scope (8,441 examples), only 6.1% of
  the targets omit it. Across the 18,918 examples with a scope list, the
  target's scope is in the list 35.9% of the time (the first one listed
  16.0%), new 30.9%, absent 33.1%.
  With no listed scope fitting `mise.toml`, the 0.5B takes the most
  frequent one, the 1.5B makes one from the file name.

- **Data.** Raise the share of targets whose type is outside the list and
  of unscoped targets under a fully scoped history, by selection in
  `select.py`, not by editing targets. Train with the scope map of H4
  (H4b) so "no scope fits" is visible in the prompt.
- **Prompt, if data is not enough.** List the standard Conventional types
  after the ones used here, so `chore` and `build` are always offered.
- **Metric.** On eval-v1, type_match restricted to commits whose type is
  not in the prompt's list, and scope_fits; the four fixtures' cases read
  by hand.
- **Expectation.** Those rates up, overall type_match within noise.

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

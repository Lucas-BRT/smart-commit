# Hypotheses and measurements

Every number comes from `sc-training score` on `data/eval-v1` (500 commits from 70
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
0.5B with nested subsets (`nest.py`, now `sc-training nest`) of 2.5k, 5k, 20k and 40k examples,
the 40k set on the 1.5B, and h3a, the 0.5B on 9,278 examples: H1's set
with the unsupported and version-only targets dropped and nothing chosen
in their place, `sc-training filter-supported data/h1 data/h3a` (first made with an
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

### H4. Showing where each scope is used fixes scopes copied from the history — prompt alone: no effect; H4b planned

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
- **Taken out of the prompt again** (`style::scope_areas` and the paths in
  traces stay): with no model trained on it, it only cost tokens. It comes
  back with H4b, switched on for the model trained with it, like H6's
  switch.
- **Measured with v0.1, map against none** (`smart-commit eval --device
  vulkan`, the binaries of `04e3713` and of the revert, on the 138 inputs
  of eval-v1h whose prompt the map changes, 136 Conventional; the other
  362 get the same prompt, hence the same reply). The map adds ~725
  characters (~200 tokens) to the system message, and the 1024-token
  budget takes them from the diff: 1081 → 695 characters on average,
  shorter in 67 of 138. Without the map, v0.1 0.5B: ungrounded 0.206 →
  0.103, word_f1 0.335 → 0.368, scope_fits 0.471 → 0.515; v0.1 1.5B:
  ungrounded 0.184 → 0.074, word_f1 0.352 → 0.395, scope_fits 0.441 →
  0.478; type_match within noise for both. The base 1.5B moves the other
  way, within noise (type_match 0.449 → 0.419, scope_fits 0.331 → 0.287).
  H4b must keep the diff's share: a shorter map (the idf idea below) or a
  budget that does not shrink the diff.
- **Weakness seen on this repository:** docs/ is among the places of almost
  every scope (each feature updates the docs here), so the map is long
  (~100 tokens for 8 scopes) and tells scopes apart less. Possible fix:
  leave out places shared by most scopes, the way idf discounts common
  words.

### H5. Training on young repositories stops the copying — confirmed in v0.2

Found in real use: a new repository with one commit (`feat: Add a simple
hello world program`) and a one-line change adding a `println!` with a sum.
Every model tried, base and fine-tuned, 0.5B and 1.5B, answered with the
history subject verbatim; H1 came closest (`... with a print statement`).

Cause in our data: `sc-training select` requires `--min-history 10`, for training
and for eval-v1, so neither has ever seen a young repository. With one
example and "follow their style", repeating it is the easy answer.

- **Guard, done on main** (`fix(generate): retry when the reply repeats a
  history subject`): a reply repeating a history subject is rejected and
  retried with a note naming it; repeated again, it falls back to the file
  list. Nothing copied reaches the user, but the fallback is generic.
- **eval-short**: young repositories, 1–5 history subjects, from test
  shards 2–5 (`sc-training select --min-history 1 --max-history 5 --per-repo 5`):
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
  `train-all` holds 86,332 such commits from 9,509 repositories, spread
  evenly from 0 to 9 subjects, 4,233 of them Conventional. Selected:
  `data/h5`, 1,600 Conventional (447 repositories) and 2,400 free (2,120),
  at most 5 per repository like eval-short (commands in
  [README.md](README.md)).
- **Prompt, if data is not enough.** With a very short history, present
  the examples as a format reference only ("follow their format, do not
  repeat them").
- **Expectation.** first reply copied and fallback on eval-short down to
  H1's level for the 0.5B, eval-v1 unchanged.

### H6. The rules can live in the weights instead of the prompt — confirmed in v0.2

Context distillation: train the model on prompts without the fixed rules
(reply with the message only, 72 columns, the imperative, the Conventional
Commits format), keeping only what varies per repository (types, scopes,
examples, language, ticket) and the diff. The behaviour the rules asked
for moves into the weights; the prompt shrinks.

- **Why not soft prompts.** Learning a few embedding vectors to replace the
  rules (prompt or prefix tuning) would save the same tokens, but neither
  llama-server nor OpenVINO GenAI accept embeddings in place of text, so it
  needs runtime changes on every backend. Context distillation needs none.
- **Gain, measured.** The estimate was 200–250 tokens of fixed rules and
  prompts 40–60% faster. Built with `prompt --inputs --rules omitted`, the
  system message loses ~368 characters (~105 tokens): 690 → 322 on eval-v1,
  542 → 184 on eval-short. Whole prompts shrink 17% and 21% (1709 → 1417
  and 1424 → 1118 characters), less than the rules alone, because the
  budget the rules freed goes to diffs that were cut (1019 → 1095
  characters on eval-v1). Prefill is about 90% of the time per message on
  the CPU and scales about linearly with tokens, so expect roughly 15–20%
  faster messages there, plus more of the diff where it was cut; measure
  with the trained model.
- **Rust, done.** `PromptRules::{Full, Omitted}` in `PromptOptions`,
  recorded in traces when not full. It follows the model file that runs:
  `Omitted` only when the GGUF is the checked file of a registry model
  whose `rules` say so (none yet), never a setting. `prompt` and `eval`
  take `--rules full|omitted` to build training sets and to evaluate a
  candidate before it is in the registry. Omitted keeps what varies by
  repository (types, scopes, examples) and what the training data lacks
  (tickets, Portuguese, gitmoji).
- **Training.** The same pipeline, with `make_examples.py` (now `sc-training make-examples`) building prompts
  with `--rules omitted`. Best trained together with H5 (young repositories) in
  one bundle, on about 20k examples, where H3's curve flattens.
- **Metrics.** eval-v1 and eval-short with the switch on against H2/H1 with
  the full prompt: first_try and fallback must not move (they are what the
  rules were for), type_match and word_f1 within noise; CPU seconds per
  message on the same 50 commits as before.
- **Risk.** A model trained without the rules depends on the training for
  the format; an untrained model with the switch on would fall apart, so
  the switch must follow the model, never be a free setting.

### H7. The lists in the prompt are read as closed because the data rarely leaves them — release bumps in v0.2's data; the rest planned

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

- **Release bumps.** `sc-training build-inputs` drops every release bump
  (`chore: release …`, version-only descriptions), so the models never saw
  one. Keep those whose version is in the diff (a `version = ` line), where
  the target is predictable; keep dropping version-only ones without it.
  Built: `sc-training build-inputs --releases` keeps only those (the filters stay
  as they are, so earlier sets rebuild the same): 7,639 of the train
  split's 7.66M commits, from 988 repositories, 1,125 Conventional (1,023
  `chore`). Selected: `data/h7`, 219 Conventional and 200 free, at most 5
  per repository (`sc-training select --releases`), about 2% of a 20k bundle.
- **Data.** Raise the share of targets whose type is outside the list and
  of unscoped targets under a fully scoped history, by selection in
  `sc-training select`, not by editing targets. Train with the scope map of H4
  (H4b) so "no scope fits" is visible in the prompt.
- **Prompt, if data is not enough.** List the standard Conventional types
  after the ones used here, so `chore` and `build` are always offered.
- **Metric.** On eval-v1, type_match restricted to commits whose type is
  not in the prompt's list, and scope_fits; the four fixtures' cases read
  by hand.
- **Expectation.** Those rates up, overall type_match within noise.
- **Headroom on eval-v1 is small**: the real type is outside the prompt's
  list for 14 of its 250 conventional commits (5.6%), and the c40k runs
  already get 7 of them (`data/eval-v1/c40k-*-lora.jsonl`); unscoped targets
  under a fully scoped history are 3, all answered without a scope. H7
  matters for repositories like this one, not for the benchmark: data
  changes only, no goal of its own for v0.2.

### H8. The type cap of the selection costs `fix` — false: it moves the `fix`/`feat` boundary

What the literature says about Conventional types:

- Only `feat` and `fix` are defined by the specification; the rest come
  from the Angular convention (commitlint's `config-conventional`).
- The types mix two axes: purpose (feat, fix, refactor, perf, style) and
  the object changed (docs, test, ci, build). When both apply, Zeng et al.
  (ICSE 2025) give the purpose priority; Li et al. dropped `perf` and
  `chore`, which annotators could not tell apart.
- On CommitSuite (EASE 2026: 10 types, 116 commits each, labels checked)
  the best LLM, DeepSeek-R1, gets 64% right and GPT-4.1 61%; ci, build and
  chore are confused with each other, perf, style and fix with refactor.
  AST context did not help the classification. Few-shot prompts help,
  chain-of-thought does not (3,200 InfluxDB commits, 2605.02033).

What eval-v1 says (250 Conventional commits, the c40k LoRA runs, files
grouped by path rules: docs, tests, CI, build files, else code):

| Files | n | base 1.5B | v0.1 0.5B | v0.1 1.5B |
|---|---|---|---|---|
| code only | 169 | 0.57 | 0.56 | 0.61 |
| mixed | 41 | 0.34 | 0.49 | 0.56 |
| docs only | 25 | 0.32 | 1.00 | 1.00 |
| tests, CI or build only | 15 | 0.13 | 0.47 | 0.67 |

- The object types are learned: docs-only commits are all right. Path
  rules would add little, and real labels do not follow them (5 of the 9
  test-only commits are `chore`).
- The errors are on purpose types: `fix`↔`feat` is 37 of the 0.5B's 104
  errors (`fix→feat` alone 30) and 25 of the 1.5B's 89; then `fix→chore`
  (10, 11) and `refactor→feat`.
- The 0.5B finds 51% of the `fix` commits and 79% of the `feat` ones. The
  selection caps each type at 30% (`sc-training select --max-type-share 0.3`). Less
  than it seemed: without the cap, `fix` is 34.9% of the same selection
  (a cap of 0.45 does not bind and gives the same commits), and the 41% of
  eval-v1 is its own: 20% to 70% across its 7 Conventional repositories.
  The cap moves `fix` by 5 points, not 11.
- `sc-training score` now reports each type's precision, recall and F1 and their
  mean, `type_macro_f1`: base 1.5B 0.172, v0.1 0.5B 0.537, v0.1 1.5B
  0.548 (type_match 0.480, 0.588, 0.648 on the same traces). Spellings of
  one type are folded (`tests` is `test`) in selection and scoring; the
  target keeps the repository's spelling.
- Not worth it here: dropping subjects with two types (`chore: refactor:
  …`), 9 of c40k's 40,000 and none of eval-v1.

- **Experiment, bundle `h8`.** The 0.5B on c20k without the cap
  (`c20k-nocap-0.5b`, 19,403 of its 20,000 commits shared with c20k)
  against c20k retrained in the same session (`c20k-ctl-0.5b`, the bytes
  b2 trained on), one per Kaggle T4, ~75 min. The control retrained also
  measures the training noise: its scores against b2's c20k-0.5b.
- **Expectation.** A small contrast: `fix` recall up a few points at most,
  likely within noise (102 `fix` commits in eval-v1, a standard error near
  0.05). If so, the cap is not what keeps `fix` low, and v0.2 can drop it
  (the base model's bias it was for is gone after fine-tuning). Report
  type_match, type_macro_f1, `fix` recall and `fix→feat`.
- **Result.** Kaggle T4, 75 min each (train loss 2.632 with the cap,
  2.647 without; validation 2.842 and 2.839). Merged and re-quantized with
  `export_gguf.py`, scored with one binary (`a5d1770`, Vulkan, RTX 3050):

  | eval-v1, Conventional | type_match | type_macro_f1 | `fix` recall | `feat` recall | fix→feat | feat→fix | first_try | fallback |
  |---|---|---|---|---|---|---|---|---|
  | c20k-0.5b of b2 (cap 0.3) | 0.632 | 0.473 | 0.735 | 0.654 | 17 | 15 | 0.936 | 0.036 |
  | c20k-ctl-0.5b (the same, retrained) | 0.632 | 0.457 | 0.755 | 0.654 | 15 | 15 | 0.936 | 0.036 |
  | c20k-nocap-0.5b | 0.628 | 0.490 | 0.775 | 0.558 | 11 | 19 | 0.964 | 0.020 |

  On eval-short the three are within noise (fallback 0.076, 0.086,
  0.076; word_f1 0.239, 0.236, 0.241).
- **The training noise is small:** the same data trained twice gives the
  same type_match and train loss to three decimals, so one run per
  variant is enough to compare.
- **The cap trades `fix` for `feat`:** without it `fix` recall gains 2–4
  points and `feat` loses about 10; the errors change side, type_match
  does not move. type_macro_f1, first_try and fallback are slightly
  better without it, within noise. The cap is dropped for v0.2: no cost,
  one arbitrary knob less, and the natural distribution of the selection.
  The `fix`/`feat` boundary needs better labels or more context, not
  another mix.

### H9. A larger prompt budget helps the trained models — a trend, within noise overall

The diff shares a 1024-token budget; a trained model may describe better
seeing more of it. v0.1 on eval-v1 with `model.max_prompt_tokens` 1536
against 1024 (Vulkan, RTX 3050, one binary): only the 109 of 500 commits
whose diff was cut get another prompt, so the other 391 give the same
reply.

| On the 109 cut commits | word_f1 | ungrounded | unsupported | type_match (50 Conventional) |
|---|---|---|---|---|
| v0.1 0.5B, 1024 → 1536 | 0.240 → 0.258 | 0.055 → 0.055 | 0.018 → 0.000 | 0.54 → 0.56 |
| v0.1 1.5B, 1024 → 1536 | 0.269 → 0.279 | 0.083 → 0.037 | 0.046 → 0.009 | 0.64 → 0.64 |

- Seeing more of the diff, the models invent fewer specifics and write
  fewer vague descriptions: consistent, but a handful of commits each.
- Over the 500, within noise: type_match +0.004 (both), type_macro_f1
  +0.03 and +0.02, word_f1 +0.008 and +0.009.
- Cost: 7–9% more time over eval-v1 on Vulkan (68 → 73 s for the 0.5B,
  127 → 139 s for the 1.5B); on the CPU, where prefill dominates, the cut
  commits' prompts grow by up to half.
- The NPU compiles for a fixed prompt length, so a budget per device or
  per model would be needed; H6 already frees about 105 tokens for the
  diff. Not pursued for now.

### H10. The 7B base model can teach the small ones — false

The labels are the limit of the type (H8); a larger model's replies could
replace them (distillation). Qwen2.5-Coder-7B, installed for users who want
it, scored on eval-v1 like the others (1024 tokens, Vulkan):

| eval-v1, Conventional | type_match | type_macro_f1 | word_f1 | ungrounded | 500 messages |
|---|---|---|---|---|---|
| base 1.5B | 0.452 | 0.183 | 0.248 | 0.188 | 149 s |
| base 7B | 0.468 | 0.365 | 0.278 | 0.116 | 414 s |
| v0.1 0.5B | 0.636 | 0.542 | 0.340 | 0.104 | 68 s |
| v0.1 1.5B | 0.656 | 0.545 | 0.367 | 0.084 | 127 s |

The 7B is better than the 1.5B base but behind both fine-tunes on every
metric, and three times slower than v0.1 1.5B: as a teacher it would make
the small models worse. Fine-tuning on the task beats four times the
parameters. Better labels have to come from elsewhere (checked datasets,
or preference training on the failure modes).

### v0.2 (bundle b3): H5, H6 and H7 together — the 0.5B gains, the 1.5B trades

b3 (`data/kaggle/b3-nocap`): 20,019 examples without the type cap (H8),
4,000 of young repositories (H5), 419 release bumps (H7), every prompt
without the fixed rules (H6), the validation set too. Kaggle T4: 71 min
for the 0.5B (train loss 2.639, validation 2.826), 148 min for the 1.5B
(2.403, 2.551); losses are not comparable with v0.1's, whose prompts had
the rules. Merged and re-quantized with `export_gguf.py`; v0.2 scored with
`--rules omitted`, v0.1 with the full rules, one binary (`94ababd`,
Vulkan, RTX 3050).

| eval-v1, Conventional | type_match | type_macro_f1 | first_try | fallback | word_f1 | ungrounded |
|---|---|---|---|---|---|---|
| v0.1 0.5B | 0.636 | 0.542 | 0.976 | 0.016 | 0.340 | 0.104 |
| v0.2 0.5B | 0.632 | 0.526 | 0.972 | 0.012 | 0.337 | 0.104 |
| v0.1 1.5B | 0.656 | 0.545 | 0.944 | 0.012 | 0.367 | 0.084 |
| v0.2 1.5B | 0.628 | 0.498 | 0.956 | 0.020 | 0.352 | 0.084 |

| eval-short | first_try | first reply copied | fallback | word_f1 | ungrounded |
|---|---|---|---|---|---|
| v0.1 0.5B | 0.863 | 0.137 | 0.086 | 0.244 | 0.132 |
| v0.2 0.5B | 0.924 | 0.076 | 0.046 | 0.233 | 0.102 |
| v0.1 1.5B | 0.878 | 0.112 | 0.030 | 0.294 | 0.102 |
| v0.2 1.5B | 0.939 | 0.056 | 0.020 | 0.274 | 0.142 |

CPU, Ryzen 5 5500, the first 50 commits of eval-v1 (`--device cpu`, load
counted once): 0.5B 1.150 → 0.976 s per message, 1.5B 2.532 → 2.156 s.

- **H5 confirmed:** in young repositories the first reply copies half as
  often and the format holds better (first_try +0.06 for both); the 0.5B
  falls back half as often (8.6% → 4.6%).
- **H6 confirmed, with the measured gain:** without the rules in the
  prompt both models are about 15% faster on the CPU, and the format did
  not suffer (first_try and fallback within noise on eval-v1).
- **The 0.5B keeps its eval-v1 quality;** the 1.5B loses a little on the
  type (−0.028, about one standard error; type_macro_f1 −0.047) and its
  descriptions are vaguer in young repositories (ungrounded +0.04).
- **The 1.5B leans on the rules more than the 0.5B.** Given the full
  prompt anyway, v0.2 1.5B scores type_match 0.644 and type_macro_f1 0.514
  (against 0.628 and 0.498 without the rules, 0.656 and 0.545 for v0.1),
  and in young repositories ungrounded 0.112 (0.142 without); v0.2 0.5B
  with the full prompt loses instead (type_match 0.612, type_macro_f1
  0.448). Twenty thousand examples without the rules may not be enough for
  the 1.5B to take them in; v0.1 1.5B had forty thousand. To try in v0.4.
- Both are public on Hugging Face and registered as experimental:
  `LucasBRT/smart-commit-0.5b-v0.2` (revision `bdb93a5`) and
  `LucasBRT/smart-commit-1.5b-v0.2` (`4bca5dc`); v0.1 stays the better
  1.5B on the type.
- Building b3 found a bug in the prompt budget's token estimate: see
  `docs/architecture.md`, Fitting the diff.

### H-split. The model can tell when the staged changes are several commits — planned

To be tried on a branch of its own once H1/H2 show the basic fine-tune
works: it changes the reply format and the Rust side.

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

### H11. An annotated file list helps models never trained on it — confirmed for the type

The plain file list (`path | +3 -1`) does not say whether a file is new,
deleted, renamed, only reformatted or only commented. `--diff annotated`
says so (src/annotate.rs). eval-v1, Vulkan, greedy, each model with its
best rules (0.5B v0.2 `--rules omitted`, the others `--rules full`), with
the near-copy and language retries and the typed fallback of the same
branch, paired with `sc-training score --against` against the plain list:

| Model | type_match plain → annotated | Paired | type_macro_f1 |
|---|---|---|---|
| smart-commit-0.5b-v0.2 | 0.636 → 0.664 | +7/−0, p = 0.016 | 0.547 → 0.562 |
| smart-commit-1.5b-v0.2 | 0.664 → 0.668 | +3/−2 | 0.550 → 0.551 |
| qwen2.5-coder-0.5b | 0.268 → 0.276 | +5/−3 | 0.167 → 0.166 |
| qwen2.5-coder-1.5b | 0.508 → 0.516 | +4/−2 | 0.228 → 0.235 |

- No model gets worse on the type, unsupported or ungrounded; the 0.5B
  fine-tune's gain is the only significant one. It is the default since.
- On the commit scenarios the 1.5B v0.2 goes from 0.51 to 0.60
  (`expected`), mostly renames and deletions; a whitespace-only file taken
  out of the diff made it invent until the list said which whitespace.
- Next: train v0.4 on the annotated list (T1); the plan is H13.

### H12. DPO on the model's own samples removes its typical failures — first run: yes, but it cost the scope; second run: yes, at no cost, but the type does not improve significantly

SFT shows what to write, never what not to. DPO trains on pairs: a reply the
model sampled that a check rejects (a history subject copied, a detail found
nowhere in the diff, a vague description), against one that passes
(`train_dpo.py`: a LoRA on the SFT model merged, which is also the
reference; `mine_pairs.py` and `sc-training pairs` make the pairs).

**First run** (v0.1 0.5B, `--rules full --diff plain`). 3,551 inputs of
`data/test2to5h` outside the evaluation sets' repositories, four samples each
at 0.7, 1,287 pairs (964 with the real subject chosen), of which only 72
Conventional (191 of the 3,551 inputs were), judged by the checks that need
no reference. β 0.1, learning rate 2e-5, 80 steps of 16 pairs, 12 minutes on the
RTX 3050. One binary, Vulkan, greedy, eval-v1 (250 + 250) and eval-short
(47 + 150), the SFT model re-exported the same way, paired:

| eval-v1 | SFT | DPO β 0.1 | DPO β 0.3 |
|---|---|---|---|
| ungrounded, Conventional / free | 0.104 / 0.108 | 0.008 / 0.012 | 0.012 / 0.016 |
| first_copied, Conventional / free | 0.040 / 0.084 | 0.016 / 0.008 | 0.020 / 0.008 |
| fallback, Conventional / free | 0.028 / 0.076 | 0.008 / 0.008 | 0.012 / 0.008 |
| first_try, Conventional / free | 0.960 / 0.916 | 0.980 / 0.992 | 0.980 / 0.992 |
| scope_known, Conventional | 0.844 | 0.796 | 0.804 |
| type_match, Conventional | 0.628 | 0.612 | 0.612 |
| word_f1, Conventional / free | 0.333 / 0.230 | 0.319 / 0.251 | 0.323 / 0.250 |

- What the pairs teach, it learns: paired on all of eval-v1, `ungrounded`
  +0/−48, `first_copied` +2/−27, `first_try` +27/−3 (all p < 0.001), the
  same on eval-short. The messages do not turn generic: they are a little
  longer (39 columns against 37; the real ones 41) and the share of their
  content words that are in none of the history, the paths and the diff
  (`novel` of `sc-training compare`) does not grow, it falls (0.147 → 0.126
  Conventional, 0.118 → 0.105 free; the real subjects 0.327 Conventional).
- What it did not teach, it lost: `scope_known` +0/−12 (p < 0.001; eval-short
  0.681 → 0.617) and `type_match` +9/−13 (p = 0.52; eval-short 0.617 → 0.702).
  β 0.3 stays closer to the SFT model and did not recover the scope, so the
  cause is the pairs, not how far the model moves.
- The pairs: 94% of them are free-style prompts. The 191 Conventional inputs
  came from 20 repositories (few repositories of the test splits are
  Conventional), and the judge ignored the type and the scope. Of those
  inputs, 78% have at least one sample of the real type, 50% have samples of
  the right and the wrong type, and 53% of the samples are right: there is
  room for pairs on the type.

**Second run.** Conventional inputs from `data/train-all`
without the commits the SFT trained on (`data/curve`) and without the
evaluation and validation repositories: 4,500 from 680 repositories (at most
8 each, 22% of a type) and 1,000 of young repositories, six samples each,
mined on Kaggle's T4s; a judge that also rejects a scope the history never
used and a type other than the real commit's, and prefers a sample of the
model itself; pairs balanced by repository, type and bucket, a tenth of the
repositories apart for validation (`sc-training balance`). A sweep of β,
learning rate, epochs and `--sft-weight` on the 0.5B chosen on `data/val1`,
never on eval-v1, which is read once, for the chosen configuration; then the
1.5B.

*Fixed before the second run.* It counts as better if, on eval-v1 and
eval-short, paired against the SFT model: `type_match` and `type_macro_f1`
rise on the Conventional set, `scope_known` is back at 0.84 or more, and
`ungrounded`, `first_copied` and `first_try` keep at least 80% of the first
run's gain; and neither `word_f1` nor `exact` falls with significance, the
share of content words found nowhere (`novel`) does not grow (written "falls"
by mistake before the first sweep; the plan said it must not grow, as the guard
against invention), the mean subject stays between 36 and 42 columns, and a reading of 30 messages side by side shows no new habit. If
not, the result goes here as it is and Kaggle's quota goes no further on it.

**Second run, first sweep: it failed, and why.** Six runs of the 0.5B on
Kaggle's T4s (3,615 pairs of 826 repositories, 514 pairs of other
repositories for validation), chosen on `data/val1` (250 Conventional + 250
free), paired against the SFT model. All six learn the validation pairs
(accuracy 0.77–0.83) and the chosen reply's reward turns negative in all
(−0.5 to −1.6; −0.05 with `--sft-weight`): the model makes both replies less
likely, the rejected one more so:

| val1, Conventional | type_match | scope_known | replies with a scope | `chore` replies (real: 35 of 250) | ungrounded | word_f1 | novel |
|---|---|---|---|---|---|---|---|
| SFT | 0.624 | 0.904 | 0.23 | 38 | 0.076 | 0.318 | 0.134 |
| β 0.1, 2e-5 | 0.436 | 1.000 | 0.00 | 141 | 0.000 | 0.301 | 0.054 |
| β 0.3, 2e-5 | 0.556 | 0.980 | 0.03 | 88 | 0.008 | 0.319 | 0.076 |
| β 0.1, 5e-5 | 0.160 | 1.000 | 0.00 | 240 | 0.004 | 0.213 | 0.041 |
| β 0.3, 5e-5 | 0.492 | 1.000 | — | — | 0.000 | 0.320 | 0.064 |
| β 0.1, 2e-5, `--sft-weight 0.2` | 0.608 | 0.972 | 0.07 | 65 | 0.004 | 0.335 | 0.070 |
| β 0.1, 1e-5, 2 epochs | 0.400 | 1.000 | — | — | 0.000 | 0.292 | 0.047 |

- It is not the type pairs that failed but what the pairs add up to. The
  rejected reply is a sample of the model, which errs most in the types it
  writes most: among the Conventional pairs `fix` is chosen 621 times and
  rejected 683, `feat` 584 and 681, `chore` 603 and 403; 33% of the chosen
  replies have a scope against 47% of the rejected. The cheapest way to
  lower the loss is to move the model's habit (fewer `fix` and `feat`, more
  `chore`, no scope) and not to learn which type each diff is. `scope_known`
  of 1.0 is that: no scope is always known.
- Only the run that keeps the chosen reply likely (`--sft-weight`) stays
  close to the SFT model. It beats it on `ungrounded`, `first_copied`,
  `first_try` and `word_f1`, loses 4 `type_match` cases (not significant)
  and still writes `chore` twice as often as the real commits.
- All six use far fewer words that are in nothing the model was given
  (`novel`: 0.134 for the SFT model, 0.04–0.08 for them, the real subjects
  0.316): they say what the diff says and not what the change means. Side
  by side (`sc-training compare --read`), for a change the real commit calls
  `feat: export getElementInfo function` the run without `--sft-weight`
  writes `chore: move matrix utils to utils`. It is not the invention the
  criterion guards against but a habit to count.
- By the criteria above none of the six is better (`type_match` does not
  rise). The mean subject of the SFT model on val1 is 35.3 columns, below the
  36–42 the criteria assumed from eval-v1.
- Next: pairs whose chosen and rejected replies have the same distribution of
  type and of scope (`sc-training balance --match-marginals`), so that only
  what depends on the diff can lower the loss, and a larger `--sft-weight`.

**Second run, second sweep: the collapse is gone.** Six runs of the 0.5B on the pairs with matched
marginals (3,172 for training of 802 repositories, the same 514 for validation), `--sft-weight` 0,
0.2, 0.5 and 1.0, on Kaggle's T4s (about 1.7 h of the session, 5.05 of the 30 h of the week in all).
Chosen on `data/val1`, paired against the SFT model:

| val1, Conventional | type_match | type_macro_f1 | scope_known | replies with a scope | novel | ungrounded | first_try (free) | word_f1 |
|---|---|---|---|---|---|---|---|---|
| SFT | 0.624 | 0.528 | 0.904 | 0.23 | 0.134 | 0.076 | 0.936 | 0.318 |
| β 0.1, 2e-05, `--sft-weight` 0 | 0.584 | 0.399 | 0.936 | 0.18 | 0.063 | 0.000 | 1.000 | 0.328 |
| β 0.1, 2e-05, `--sft-weight` 0.2 | 0.628 | 0.517 | 0.924 | 0.20 | 0.075 | 0.008 | 0.988 | 0.346 |
| β 0.1, 2e-05, `--sft-weight` 0.5 | 0.628 | 0.515 | 0.924 | 0.21 | 0.086 | 0.020 | 0.980 | 0.339 |
| β 0.1, 2e-05, `--sft-weight` 1 | 0.636 | 0.518 | 0.928 | 0.21 | 0.085 | 0.028 | 0.976 | 0.341 |
| β 0.1, 1e-05, `--sft-weight` 0.5 | 0.640 | 0.521 | 0.904 | 0.24 | 0.087 | 0.024 | 0.976 | 0.341 |
| β 0.3, 2e-05, `--sft-weight` 0.5 | 0.632 | 0.514 | 0.928 | 0.21 | 0.081 | 0.012 | 0.976 | 0.343 |

- The matching alone is not enough: without `--sft-weight` the chosen reply still becomes less
  likely (validation reward −0.446, against +0.08 to +0.13 with 0.5 or more) and the type falls
  (0.584, `type_macro_f1` 0.399). With it the type, the scope and the habits stay where the SFT
  model has them (in the first sweep the runs wrote `chore` for 141 to 240 of the 250 commits) and the
  failures the pairs show go: `ungrounded` +12 to +19, `first_copied` +5 to +8, `first_try` +5 to +8 cases.
- The type is flat on `val1` for the five runs with `--sft-weight` (+1 to +4 cases, none significant; `type_macro_f1` 0.514–0.521 against
  0.528). `novel` falls to a half or two thirds again (0.134 → 0.075–0.087; the real commits 0.316).
  A reading of 30 cases side by side shows replies close to the SFT model's, a complement dropped or a
  verb changed, and no new habit.
- Run chosen on `val1`: **m-b01-lr2-s02** (β 0.1, 2e-5, `--sft-weight` 0.2), the one with the most
  significant gains and the highest `word_f1` (0.346). The scenarios (14 Conventional) lose one case of
  `type_match` and one or two of `expected` in every DPO run, and are even or a little better on the 29
  free ones (`expected` −1 to +2).

Read once on the evaluation sets, paired against the SFT model:

| | SFT → m-b01-lr2-s02 | paired |
|---|---|---|
| eval-v1 Conventional, `type_match` | 0.628 → 0.664 | +15/−6, p = 0.078 |
| eval-v1 Conventional, `type_macro_f1` | 0.531 → 0.557 | |
| eval-v1 Conventional, `scope_known`; replies with a scope | 0.844 → 0.840; 0.396 → 0.380 | +4/−5 |
| eval-v1 Conventional, `ungrounded`; `first_copied`; `first_try`; `fallback` | 0.104 → 0.008; 0.040 → 0.004; 0.960 → 0.996; 0.028 → 0.004 | all p < 0.05 |
| eval-v1 free, `ungrounded`; `first_copied`; `first_try`; `fallback` | 0.108 → 0.008; 0.084 → 0.004; 0.916 → 0.996; 0.076 → 0.004 | all p < 0.001 |
| eval-v1, `word_f1` Conventional, free; `novel` Conventional | 0.333 → 0.347, 0.230 → 0.247; 0.147 → 0.078 | |
| eval-short Conventional (47), `type_match`; `type_macro_f1`; `scope_known` | 0.617 → 0.617; 0.669 → 0.609; 0.681 → 0.617 | +3/−3; ; +0/−3 |
| eval-short free (150), `first_try`; `first_copied` | 0.840 → 1.000; 0.160 → 0.000 | p < 0.001 |

- **Against the criteria fixed above:** met on eval-v1 (the type and its macro F1 rise, though the
  `type_match` not with significance; `scope_known` is 0.840; the gains of the first run are kept or
  exceeded; `word_f1`, `exact` and `novel` are fine; the Conventional subjects average 40.2 columns);
  **not met on eval-short**, where `type_macro_f1` falls and `scope_known` loses three cases, on 47
  commits. Taken together: the DPO now removes what the first run removed, at no cost to the type and the
  scope, and does **not** improve the type significantly, which was the aim of the second run. One seed
  per configuration, so differences of a few cases between runs are noise.
- The six runs are in a private repository, `LucasBRT/smart-commit-0.5b-dpo-runs` (adapters, logs,
  `run.json`, Q4_K_M GGUF and the results), as experiments and not as a version of the model.
- Next, by decision of the author and not because the criteria were met: the same configuration on the
  1.5B (below).

**Second run on the 1.5B.** The same recipe on `smart-commit-1.5b-v0.1`: 3,091 pairs (885 of
free-style commits, 795 repositories) with matched marginals, 500 for validation, β 0.1, lr 2e-5, one epoch
(193 steps), two runs of `--sft-weight` 0.2 and 0.5, one on each T4 of the same Kaggle session (82 minutes
each, 4.4 GiB; about 1.5 h of the session). Validation accuracy 0.754 and 0.740.
Chosen on `data/val1` (never on eval-v1), paired against the SFT model, 250 Conventional commits:

| val1, Conventional | type_match | type_macro_f1 | scope_known | novel | ungrounded | first_copied | word_f1 |
|---|---|---|---|---|---|---|---|
| SFT | 0.636 | 0.432 | 0.864 | 0.157 | 0.072 | 0.056 | 0.342 |
| `--sft-weight` 0.2 | 0.628 | 0.429 | 0.868 | 0.121 | 0.012 | 0.000 | 0.338 |
| `--sft-weight` 0.5 | 0.636 | 0.461 | 0.868 | 0.114 | 0.028 | 0.004 | 0.344 |

Both remove the failures (`ungrounded` +15 and +11 cases net, `first_copied` +14 and +13, all p < 0.01) and
leave the type and the scope where they were; only 0.5 raises `type_macro_f1`. Run chosen, **m15-b01-lr2-s05**,
for that, for the highest `word_f1` and the lowest `novel` of the two; the rule used for the 0.5B (the most
significant gains and the highest `word_f1`) points to either, and the choice was written down before eval-v1
was read. A reading of the cases side by side shows replies close to the SFT model's (a complement dropped, a
verb or a scope changed) and no new habit.

Read once on the evaluation sets (smart-commit 0.9.0 plus `main` up to `236d51f`, as for the 0.5B), paired
against the SFT model, the base model as a control:

| | SFT → m15-b01-lr2-s05, plain list | annotated list | paired (plain) |
|---|---|---|---|
| eval-v1 Conventional, `type_match` | 0.660 → 0.692 | 0.676 → 0.696 | +11/−3, p = 0.057 (annotated +9/−4, p = 0.27) |
| eval-v1 Conventional, `type_macro_f1` | 0.568 → 0.589 | 0.581 → 0.604 | |
| eval-v1 Conventional, `scope_known`; replies with a scope | 0.848 → 0.864; 0.388 → 0.384 | 0.852 → 0.868 | +6/−2, p = 0.29 |
| eval-v1 Conventional, `ungrounded`; `first_copied`; `first_try`; `fallback` | 0.068 → 0.020; 0.076 → 0.004; 0.924 → 0.996; 0.024 → 0.004 | 0.076 → 0.020; 0.072 → 0.004; 0.928 → 0.996; 0.028 → 0.004 | p < 0.001 for the first three; fallback p = 0.06 |
| eval-v1 free, `ungrounded`; `first_copied`; `first_try`; `fallback` | 0.068 → 0.016; 0.104 → 0.004; 0.896 → 0.996; 0.032 → 0.004 | 0.056 → 0.024; 0.112 → 0.004; 0.888 → 0.996; 0.036 → 0.004 | p ≤ 0.016 (annotated `ungrounded` p = 0.057) |
| eval-v1, `word_f1` Conventional, free; `novel` Conventional | 0.360 → 0.366, 0.255 → 0.265; 0.157 → 0.136 | 0.362 → 0.366, 0.256 → 0.266; 0.145 → 0.130 | |
| eval-short Conventional (47), `type_match`; `type_macro_f1`; `scope_known` | 0.723 → 0.681; 0.721 → 0.712; 0.596 → 0.638 | 0.681 → 0.702; 0.674 → 0.739; 0.553 → 0.638 | +2/−4, p = 0.69 (annotated +2/−1) |
| eval-short free (150), `first_try`; `first_copied` | 0.873 → 0.993; 0.120 → 0.000 | 0.867 → 0.993; 0.127 → 0.000 | p < 0.001 |
| base Qwen2.5-Coder-1.5B, same binary: eval-v1 Conventional `type_match`; `fallback` | 0.428; 0.060 | 0.436; 0.056 | |

- **Against the criteria fixed above:** met on eval-v1 (the type and its macro F1 rise, the type not with
  significance; `scope_known` is 0.864; `ungrounded`, `first_copied` and `first_try` are as good as the 0.5B's;
  `word_f1`, `exact` and `novel` are fine); the Conventional subjects average 42.7 columns, 0.2 above the SFT
  model's 42.3 and above the 36–42 the criteria set, which the SFT model already was. On eval-short, with
  47 commits, the plain list loses 2 net cases of `type_match` and 0.009 of `type_macro_f1` and the annotated
  one gains 1 and 0.065; `scope_known` rises with both. Not the fall of the 0.5B. Taken together: at 1.5B the
  DPO removes the same failures and the type rises a little, without significance (p = 0.057 with the list the
  model was trained on). One seed per configuration; differences of a few cases are noise.
- The GGUF of the model is `smart-commit-1.5b-v0.3` on Hugging Face, public like the 0.5B's.

### H13. The type is limited by what the model is trained on, not by the failures DPO removes — planned for v0.4

Where the models stand (v0.3, eval-v1, Conventional, default file list):
`type_match` 0.668 (0.5B) and 0.696 (1.5B), `type_macro_f1` 0.604 for the
1.5B, against the failures DPO removed (`ungrounded` 0.020, `first_copied`
0.004, `first_try` 0.996). The type has not moved since H1: SFT on more
data (H3), without the rules (H6), without the type cap (H8) and DPO on
its own samples (H12) all left it between 0.63 and 0.70. What is left that
the models have not been given:

1. **The list they will see.** Every model was trained on `--diff plain`;
   the default since H11 is `annotated`, which raised the 0.5B v0.2's type
   by 0.028 (p = 0.016) with no training on it.
2. **The scopes.** `scope_known` is fine (0.84–0.87), but "which scope fits"
   is still read from the history (H4: the prompt alone did not help). H4b
   trains with the map of scopes in the prompt.
3. **The labels.** H3: cleaner labels helped more than volume. Commits whose
   type the diff cannot tell (a `fix` or a `feat` for the same change) ask
   the model to guess a coin. How many of the type errors they are has not
   been counted; S3 starts by counting them on `data/val1`.
4. **The size of the data for the 1.5B** (H6: twenty thousand examples
   without the rules may not be enough; v0.1 had forty thousand).
5. **How the pairs weigh the type.** The pairs of H12 were balanced so that
   the type's marginal does not move (the collapse onto `chore` of the first
   sweep); the type error is one failure among several in them.

**Plan.** Cheap arms on the 0.5B first, chosen on `data/val1`; the best
recipe then on the 1.5B; DPO last, on the best SFT model. Each arm is
`--rules full`, one seed, with the data of v0.1 unless it says otherwise.

| Arm | What changes | Question |
|---|---|---|
| S0 | the v0.1 recipe (`--diff plain`) with today's prompt | the control: the prompt changed since v0.1 (the diff budget, the types listed), so S1 against v0.1 would mix both |
| S1 | trained on `--diff annotated` (S0's data, only the file list differs: 15,957 of the 40,000 examples have annotations) | does the list it is used with beat the one it was trained on? |
| S2 | S1 + the scope map of H4b | does the map fix the scope without costing the diff's share of the prompt? |
| S3 (not run as written, see below) | S2 + labels: the commits whose type the diff cannot tell, found by where the SFT model and the real type disagree on the training set, left out or relabelled after a reading; rare types (`perf`, `refactor`, `docs`, `build`) raised | do cleaner and more even labels move the type, as in H3? |
| D1 | DPO (H12's recipe) on the best of S1–S3, pairs mined from it, the type failure weighted in the pairs while the marginals still match | does DPO keep what the SFT arm gained and still remove the failures? |
| C | only if S and D fail on the type: a separate type classifier (the first token's loss, or a small model on the diff) to choose the type before the message, which the product already supports (`type_choice`, the typed start) | is the type a different problem from the message? |

C is not planned before the others are read: it changes the product, and
it is the first thing to try if S and D leave the type where it is. A
7B trained on the same data as a teacher (H10 showed the base 7B cannot
teach) is the other option and the most expensive.

*Fixed before the first run.* A v0.4 counts as better if, on eval-v1 and
eval-short, with the default file list, paired against the v0.3 of the same
size: `type_match` rises by at least 0.04 (about ten net cases of 250) with
p < 0.05 on the Conventional set, `type_macro_f1` does not fall,
`scope_known` is not lower than the v0.3's, `ungrounded` stays at 0.03 or
less, `first_copied` at 0.01 or less and `first_try` at 0.99 or more,
`novel` does not grow by more than 0.02, neither `word_f1` nor `exact`
falls with significance, `type_match` on eval-short (47) does not fall with
significance, the mean subject is at most a column longer than the v0.3's,
and a reading of 30 messages side by side shows no new habit. The arms S1
to S3 are chosen on `data/val1` by the same measures, S1 against S0 (both read
with the annotated list), S2 and S3 against the arm before, never on eval-v1,
which is read once, for the recipe that goes to the 1.5B and again for the final models. If the
type does not rise, the result goes here as it is, and C is decided then, not
before; Kaggle's quota does not go further on S and D.

*Cost.* The 0.5B arms are SFT runs like H3's: v0.1's 40k run took 2.45 h on
one T4, and S0 and S1 run together, one on each of the session's two T4s.
Their validation loss is logged on one set (S1's, annotated) and is not
compared between the two: the arms are compared on `data/val1` with
`sc-training compare`. The time of each arm is written here when it ends.

**S0 and S1 (first result).** The same 40k commits of v0.1, `--rules full`, QLoRA as v0.1, one seed,
one on each T4 of a session (2.48 h and 2.39 h, 2.0 h of the session; the quota 6.98 → 8.99 of 30 h).
`data/val1-h4` (250 Conventional + 250 free; the paths of the history are in it and the lists ignore
them), `--rules full --diff annotated`, S1 paired against S0, Conventional:

| | type_match | type_macro_f1 | scope_known | ungrounded | first_copied | first_try | fallback | word_f1 | novel |
|---|---|---|---|---|---|---|---|---|---|
| S0 (plain) | 0.632 | 0.426 | 0.884 | 0.088 | 0.048 | 0.948 | 0.028 | 0.317 | 0.138 |
| S1 (annotated) | 0.628 | 0.426 | 0.884 | 0.080 | 0.056 | 0.944 | 0.028 | 0.320 | 0.135 |
| paired | +9/−10, p = 1.0 | | +2/−2 | +9/−11, p = 0.82 | +5/−3 | +4/−5 | +5/−5 | | |

- **Training on the annotated list does not move the type** (nor anything else) on `val1-h4`: S1 is not
  better than S0 by the criteria above. On the 14 + 29 commit scenarios S1 loses a type match and
  `type_macro_f1` (0.564 → 0.485), one case. The list helps models that never saw it (H11), and
  gains nothing from being trained on, at 0.5B.
- **Where the type is wrong** (S1, `val1-h4`, 93 of 250): `feat` written `fix` 19 times, `fix` written
  `feat` 9 (together 30% of the errors); `chore` is on one side of 31 (`chore` → `fix` 11); `perf` (10
  real) is never written and `refactor` 3 times for 9 real; `fix` is written 98 times for 75 real. The
  training data has 131 `perf` of 24,000 Conventional commits (0.5%; 4% in `val1`) and 2,398 `refactor`.
  Part of the errors is the labels (`fix`/`feat`/`chore` for the same change); the rare types are not
  written at all.
- **Consequence for S2.** S2 is compared with S1 (the list the product uses); the type should not move
  with the map, which is about the scope (`scope_known` is already 0.88), so S2 is read on `scope_fits`,
  and on the type only to see that it does not fall.
- **S3 as written is not run.** The first form it took, raising the rare types, looked at the
  frequencies: in the 625,467 typed commits of `train-all` `perf` is 0.49% and `style` 0.98%, and the
  training set has 0.5% and 1.2%: the model was trained on the natural shares. `perf` is 4% of `val1`
  and 2% of eval-v1 (5 of 250), a property of those samples; raising it to 4% would fit the model to
  them and make it write `perf` where the repositories rarely have it, which is what H8 found with the
  other cap (the boundary moves). `refactor` is 10% of the training data and written a third of the
  times it should be: not a matter of frequency either. Cleaner labels (dropping the commits whose type
  the diff cannot tell) stay for later, since they need a model's disagreement with the real type and
  a reading, and nothing here says how many there are.
- **S0r.** The second T4 of S2's session runs S0 again with `--seed 1`: how much `type_match` moves
  with the seed alone, which every difference between arms (a few cases of 250) has to be read against.

**S2 and S0r (second result).** S2 (the annotated list and the scope map, 40k commits with the paths of
their history) and S0 again on seed 1, the two T4s of one session (2.53 h and 2.45 h; the quota 8.99 →
11.57 of 30 h). `data/val1-h4` and `data/scenarios-eval`, `--rules full --diff annotated`, S2 read with
the map and the others with the names (`compare`'s `NAME:map=GGUF`), Conventional:

| val1-h4 | type_match | type_macro_f1 | scope_known | scope_fits | ungrounded | first_try | word_f1 | novel |
|---|---|---|---|---|---|---|---|---|
| S0 | 0.632 | 0.426 | 0.884 | 0.700 | 0.088 | 0.948 | 0.317 | 0.138 |
| S0r (seed 1) | 0.624 | 0.466 | 0.884 | 0.692 | 0.100 | 0.944 | 0.323 | 0.146 |
| S1 | 0.628 | 0.426 | 0.884 | 0.708 | 0.080 | 0.944 | 0.320 | 0.135 |
| S2 | 0.636 | 0.440 | 0.896 | 0.700 | 0.080 | 0.956 | 0.320 | 0.116 |

- **The seed alone moves the type as much as the arms do.** S0r against S0: `type_match` +10/−12 cases,
  `type_macro_f1` +0.040, `scope_fits` +1/−3, `ungrounded` +8/−5. Every difference between S0, S1 and S2
  is inside that.
- **S2 against S1** (the arm before it): `scope_fits` 0.700 against 0.708 (+2/−4, p = 0.69),
  `scope_known` +5/−2 (p = 0.45), `type_match` +9/−7. The map does not improve the scope at 0.5B on this
  set, where `scope_fits` is already 0.70 and the real subjects 0.752 (H4). On the free commits S2 copies
  an earlier subject in 6 more cases and 1 fewer (p = 0.125), the one movement worth watching.
- **Against the criteria fixed above:** neither S1 nor S2 is better than the arm before it, and eval-v1
  is not read: there is no recipe to carry to the 1.5B. The type stays where H1 left it, 0.63 at 0.5B on
  this set.
- **What this says about the type.** More data (H3), no rules (H6), no type cap (H8), DPO (H12), the list
  it is used with (S1) and the scope map (S2) leave the 0.5B's `type_match` between 0.62 and 0.67, and the
  seed alone moves it by 2 points. The errors are `fix`/`feat`/`chore` for the same change in a third of
  the cases each; the next steps in the plan are the DPO with the type weighed in the pairs (D1) and,
  failing it, a type chosen apart from the message (C), which the product can already ask the user for.

**C, probed before building it.** A plain classifier on the same signals (`type_probe.py`: the paths, the file
statuses, the words of the added and removed lines, the line counts and the types the history uses; TF-IDF
and a logistic regression, seconds on the CPU), trained on up to 200,000 Conventional commits of `train-all`
(751 repositories, none of the evaluation ones, at most 500 per repository) and read on the 250 Conventional
commits of `data/val1-h4`, like the models:

| training commits | type_match | type_macro_f1 |
|---|---|---|
| 24,000 | 0.576–0.580 | 0.376–0.387 |
| 80,000 | 0.608–0.612 | 0.388–0.390 |
| 200,000 | 0.604–0.612 | 0.390–0.397 |

- **It stops at 0.61, whatever the data.** The models read on the same set write 0.620 (0.5B v0.3) and 0.648
  (1.5B v0.3): a bag of words nearly matches a language model on this task, so the signals in the diff and
  the history do not say more than that, and the rest is the labels.
- **They are right in different cases, and a combination does not use it.** The classifier and the 1.5B v0.3
  agree on 68% of the commits; at least one is right in 74% (an upper bound, not a result). Letting the
  classifier override the model when it is sure changes at most 7 commits (confidence ≥ 0.6, never higher)
  and gains 2 (0.648 → 0.656), where one more is noise.
- **Verdict on C.** A separate type chosen by a classifier of this kind would not raise the type: the 0.5B's
  and the 1.5B's 0.62–0.65 are near what these signals give. A stronger classifier (an encoder fine-tuned on
  the diff) is not excluded, but nothing here suggests that the room is large.

**The types of the commits on the same files, probed too.** The prompt gives the last subjects and (with
`--scopes map`) where each scope was used, but never the type of the past commits that touched the files being
committed. `type_probe.py --by-file` adds them as features (`same:` the files were touched, `near:` the
directory was, `latest:` the most recent commit on the files), from the history's file lists:

| features | type_match (80,000 commits, alpha 1e-5 / 1e-6) | type_macro_f1 |
|---|---|---|
| paths, status, words, history types | 0.612 / 0.592 | 0.388 / 0.385 |
| plus the types by file | 0.592 / 0.608 | 0.384 / 0.400 |

- **No gain.** 164 of the 250 commits have a past commit on the same files, but the type of the latest one
  equals the commit's in only 75 of them (46%), less than the model gets: in a repository the type follows
  what the change does, not where it is.
- **Verdict.** The history by file is not a lever for the type either, so no prompt or training arm is built
  on it. The signals the prompt already carries are used up; what remains is the label.

**The type, measured against more than the label.** `type_match` counts the repository's own type as the only
right one. `type_review.py` reads the traces of `compare-v04c` (the 250 Conventional commits of `data/val1-h4`)
and adds measures that tolerate an answer the label did not choose; the groups and the threshold were fixed
before reading any number: {chore, build, ci}, {refactor, perf, style}, {test, tests}; `behaviour` also merges
`feat` and `fix`; `plausible` is the label's type or one a plain classifier (`type_probe.py`, 200,000 commits)
gives at least 0.15 of the probability. Brackets are 95% bootstrap intervals (2,000 resamples).

| model | strict | group | behaviour | plausible | type in the history or the label's |
|---|---|---|---|---|---|
| 1.5B v0.3 | 0.648 [0.59, 0.71] | 0.660 | 0.752 [0.70, 0.80] | 0.904 [0.86, 0.94] | 0.964 |
| 0.5B v0.3 | 0.620 [0.56, 0.68] | 0.636 | 0.764 [0.71, 0.81] | 0.916 [0.88, 0.95] | 0.980 |
| 1.5B v0.1 | 0.636 [0.58, 0.69] | 0.644 | 0.748 [0.70, 0.80] | 0.900 [0.86, 0.94] | 0.972 |
| S0 | 0.628 [0.57, 0.69] | 0.648 | 0.756 [0.70, 0.80] | 0.920 [0.88, 0.95] | 0.972 |
| S1 | 0.628 [0.57, 0.69] | 0.648 | 0.760 [0.71, 0.81] | 0.936 [0.90, 0.96] | 0.980 |
| S2 | 0.636 [0.58, 0.70] | 0.652 | 0.748 [0.70, 0.80] | 0.940 [0.91, 0.97] | 0.980 |
| a type drawn from the real mix | – | 0.221 | – | 0.512 | – |

- **The models are indistinguishable on every one of them**, as on the strict one: the intervals overlap
  everywhere, and the pairs differ by a handful of commits (the 1.5B v0.3 is right where S2 is not in 23
  commits and the other way in 20).
- **About 10 points of the strict error are `feat` against `fix`** (0.65 to 0.75 when they count as one); the
  groups of unlike types move it by 1–2 points. The most frequent mistakes of the 1.5B v0.3 are `feat` written
  `fix` (16 of 88), `feat` written `chore` (8) and `fix` written `feat` (7).
- **The label is a type the history does not use in only 4% of the commits** (3 to 6%), so the models are not
  asked for a type they cannot know; with the label in the history the strict match is 0.66, with the label
  outside it 0.30 (10 commits, wide).
- **In 15% of the commits all six models agree on a type that is not the label** (10 to 19%), and at least
  one of them has the label in 76%: a floor for how often the repository's type is the odd one out, and
  a ceiling on what picking among these models could gain.
- **The mix of types is close to the real one** (half the L1 distance 0.10–0.13), as the earlier error matrix
  showed: `perf` and `refactor` are written too little, `fix` and `chore` too much.

*Rated by hand.* 60 of the 1.5B v0.3's 88 mistakes, up to four of each (real type, written type) pair so that
the rare ones show (`ratings-h13-type.json`, with the hash of each commit; the hunks for reading them are
rebuilt by `type_review.py --sheet`). Each got `A` if a maintainer reading the diff could reasonably have used
the written type, `N` if the type misdescribes the change, `?` if the excerpt (350 characters a file) does not
say, as for new files. **The rater is Claude, not a person**: the ratings have the bias of a language model
reading short excerpts, and a human second opinion on a sample is still to be done.

| rating | commits |
|---|---|
| A (acceptable) | 28 |
| N (misdescribes) | 16 |
| ? (cannot tell) | 16 |

- **64% of the decided mistakes are acceptable** (28 of 44); weighted by how often each pair happens, 67%.
  The `N` are mostly a type that contradicts what the diff does (a new capability written `fix`, a
  removed parameter written `fix` where `refactor` fits, `docs` or `test` on a commit that also changes code,
  a reply with no type).
- **Counting the acceptable ones, the type is right in 0.81 to 0.92 of the commits** (the 162 exact, plus the
  41 acceptable mistakes of the 88, plus the 26 that cannot be told for the upper figure), against 0.648
  for the label alone.
- **Verdict.** The type of the models is not as bad as the label says, and it does not differ between them. The
  strict `type_match` cannot tell the arms apart because its noise (labels and seeds) is larger than any
  gain they had, which is why none of the earlier arms stood out. A model change would be judged by `plausible`
  and `behaviour` together with the strict match, and the `N` that remain (about 8% of the commits by the
  weighted estimate, and another 10% that cannot be told) are what there is left to fix.

**A rule between the type and the files, and the replies with no type (looked at, not built).** Fixed before the
numbers: `docs` only if every changed file is documentation (`.md`, `.mdx`, `.rst`, `.txt`, a `docs/` directory,
README, LICENSE, CHANGELOG), `test` only if every file is a test (`test/`, `spec/`, `*.test.*`, `*.spec.*`,
`.bats`, snapshots). On the 250 commits of `data/val1-h4`:

- **Real labels.** When every file is documentation the label is `docs` in 27 of 29; when every file is a test it
  is `test` in 10 of 16. The docs rule agrees with how repositories write, the test rule does not.
- **The models break the docs rule in 4 to 10 commits each** (10 for the 1.5B v0.3, 5 for the 0.5B). The label is
  `docs` in 2 of the 10, and among the other 8 several are types a maintainer could use (a story file, JSDoc
  comments, a docs package under `packages/*-docs/`, which the paths do not show). Only about 4 are plain
  mistakes (`docs` for a README plus a code change, for the stories of a test, for typings). A veto that
  retries would fix at most those 4, 1.6% of the commits, and would also hit commits like the ones rated `A`,
  with a retry whose answer is not known to be better. The reverse (all files documentation and the model
  writes another type) happens once for the 1.5B, and the label was `docs`: too rare to matter.
- **Replies with no type: 2 or 3 commits per model, always the same ones.** They come from repositories whose
  history is not Conventional (`fix/small_talk_skill: ...`, `add jenkinsfile`), where the model follows the
  history, as the prompt asks, or the file-list fallback of an unusable reply. This is not a defect.
- **Verdict.** Neither pays: a type-and-files rule would gain a few commits at most, with the risk of losing
  acceptable ones, and the replies with no type follow the repository's own style. No product change.

**A, how far the model's confidence in the type can be trusted.** `model.ask_type` asks the person when the
probability of the type a reply starts with is low; `docs/measurements.md` measured it for the 1.5B v0.1 on eval-v1
(AUROC 0.720). The models that are recommended now are the DPO ones (v0.3), and DPO tends to make a model surer
than it is right, so `type_confidence.py` reads the same probabilities (first token, `top_logprobs`, the prompts
of the traces of `compare-v04c`, `vulkan` on the RTX) for the 250 Conventional commits of `data/val1-h4`.
Decided before the numbers: calibrated enough if ECE ≤ 0.08 and AUROC ≥ 0.70 with the share right rising with the
confidence; mis-calibrated if ECE > 0.12 or AUROC < 0.65, which would mean retuning `model.ask_type_below` and
training the type with soft labels; in between, only recorded.

| model | top1 | top2 | top3 | ECE | AUROC [95%] | right below 0.5 / 0.5–0.7 / 0.7–0.9 / 0.9+ |
|---|---|---|---|---|---|---|
| 1.5B v0.1 | 0.644 | 0.832 | 0.928 | 0.036 | 0.775 [0.71, 0.83] | 0.384 / 0.589 / 0.828 / 0.925 |
| 1.5B v0.3 | 0.652 | 0.828 | 0.924 | 0.085 | 0.764 [0.71, 0.82] | 0.439 / 0.464 / 0.770 / 0.937 |
| 0.5B v0.1 | 0.644 | 0.820 | 0.888 | 0.068 | 0.765 [0.70, 0.82] | 0.368 / 0.667 / 0.841 / 0.968 |
| 0.5B v0.3 | 0.616 | 0.824 | 0.904 | 0.063 | 0.795 [0.73, 0.85] | 0.318 / 0.529 / 0.773 / 0.974 |
| 0.5B S1 | 0.648 | 0.816 | 0.892 | 0.060 | 0.771 [0.71, 0.83] | 0.333 / 0.643 / 0.781 / 0.939 |

Asking below a confidence (share of commits asked, then the type right if the person picks the author's type
when it is among the three likeliest):

| model | below 0.5 | below 0.6 | below 0.7 |
|---|---|---|---|
| 1.5B v0.1 | 29% → 0.780 | 48% → 0.848 | 58% → 0.880 |
| 1.5B v0.3 | 23% → 0.740 | 38% → 0.804 | 50% → 0.860 |
| 0.5B v0.3 | 26% → 0.732 | 43% → 0.796 | 54% → 0.836 |

- **The confidence is informative in all five and no model is mis-calibrated by the criteria.** AUROC is 0.76 to
  0.80 (0.72 for the 1.5B v0.1 on eval-v1), the share right rises with the confidence for each, and the ECE is
  0.04 to 0.09 (the 1.5B v0.3 is the one above 0.08: in between). The ECE of 250 commits is biased upward by
  resampling (the intervals do not centre on the estimate), so the differences between models are within noise.
- **DPO did not damage the confidence.** The 1.5B v0.3 is a little worse in the 0.5–0.7 band (0.464 right where
  0.5 to 0.7 is claimed) and as sure as before at 0.9 and above; asking at the same share of commits gains
  the same (23% asked reach 0.740 where the v0.1 reaches 0.780 at 29%; by interpolation both are near 0.81 at 38%).
- **Two types cover 0.82–0.83 and three cover 0.89–0.93**, so offering up to four types (as the product does) leaves
  little outside the offer.
- **Verdict.** Nothing to retune and no reason to train the type with soft labels on this evidence: the models
  already know when they are unsure, and asking is where the type gains (0.65 to 0.74 at 23% asked, 0.86 at
  50%). The 0.50 default asks about a quarter of the commits.
- **Limits.** One set (250 commits, `val1-h4`; the earlier figures are eval-v1), the person who always picks the
  author's type, `vulkan` only. In 234 of the 250 commits the likeliest first token is the type the reply
  ended with (the other 16 are retries and replies with no type).

**A, two kinds of doubt, and a menu for the split one (tried, not shipped).** The confidence in the likeliest
type does not tell a model split between two types from one with no idea. On `data/val1-h4` (exploratory, 10 to
48 commits a model): when the two likeliest types hold at least 0.85 together and are less than 0.25 apart, the
real type is one of the two in 0.89 to 0.95 of the commits (the likeliest alone is right in 0.50 to 0.69); when
they hold less than 0.70 it is one of the two in 0.45 to 0.62 only. Margin and entropy predict a right type no
better than the plain confidence (AUROC 0.74 to 0.79 against 0.76 to 0.80). When five models give the
likeliest type, 171 of 250 commits get the same one from all (right 0.75); in 68 they split between two types,
the majority is right 0.40 and one of them 0.75.

*Test.* An adaptive menu offers only those two types when the model is split, and the present menu otherwise
(up to four types). Fixed before the held-out set was read (`eval-v1`, 250 Conventional commits, where the rule
was not tuned): worth offering if the type right (the person picks the author's type when it is offered) falls
by at most 0.010 and the menu is at least 0.3 lines shorter, for both v0.3 models and asking below 0.5, 0.6 and
0.7. `type_confidence.py menus`, `vulkan`, with the types the prompt lists as allowed:

| set | model | ask below | asked | present: lines, type right | adaptive: lines, type right |
|---|---|---|---|---|---|
| eval-v1 | 1.5B v0.3 | 0.5 | 0.160 | 3.83, 0.788 | 3.70, 0.784 |
| | | 0.6 | 0.272 | 3.78, 0.824 | 3.57, 0.816 |
| | | 0.7 | 0.412 | 3.62, 0.880 | 3.49, 0.872 |
| | 0.5B v0.3 | 0.5 | 0.248 | 3.90, 0.780 | 3.69, 0.776 |
| | | 0.6 | 0.388 | 3.76, 0.836 | 3.45, 0.824 |
| | | 0.7 | 0.544 | 3.67, 0.888 | 3.45, 0.876 |

(On `val1-h4`, where the rule was found, the menu is 0.27 to 0.46 lines shorter and the type right falls 0 to 0.004.)

- **It fails on both counts.** The 0.5B loses 0.012 at 0.6 and 0.7, over the limit, and the menus shrink by 0.13
  to 0.31 lines, under it in five of six rows. The present menu already shows 3.6 to 3.9 lines because most asks
  are not split, and the split ones are a minority of the asked commits.
- **Verdict.** Not built: a gain of a fraction of a line is not worth a rule that loses acceptable choices. The
  menu is long because the doubt is mostly diffuse.

*A cap of three types.* The three likeliest types hold 0.89 to 0.93 of the real ones, so offering at most three
shortens every menu. Fixed before reading it, on the same held-out set and asks: worth offering if the type right
falls by at most 0.010 and the menu is at least 0.5 lines shorter, for both v0.3 models and the three limits.

| eval-v1 | ask below | present: lines, type right | at most three: lines, type right |
|---|---|---|---|
| 1.5B v0.3 | 0.5 | 3.83, 0.788 | 3.00, 0.780 |
| | 0.6 | 3.78, 0.824 | 2.99, 0.816 |
| | 0.7 | 3.62, 0.880 | 2.94, 0.868 |
| 0.5B v0.3 | 0.5 | 3.90, 0.780 | 3.00, 0.760 |
| | 0.6 | 3.76, 0.836 | 2.98, 0.812 |
| | 0.7 | 3.67, 0.888 | 2.95, 0.860 |

- **The menu is 0.7 to 0.9 lines shorter, as wanted, and the type right falls too much:** 0.008 to 0.012 for the
  1.5B (over the limit at 0.7) and 0.020 to 0.028 for the 0.5B, 2 to 7 commits of 250. On `val1-h4` it falls
  0.008 to 0.020.
- **Verdict.** Not built. The fourth type is the real one (and the only place it is offered) in 3% to 8%
  of the commits asked (2 of 40 and 3 of 103 for the 1.5B, 5 of 62 and 7 of 136 for the 0.5B), often enough that
  cutting it costs more than the line saves. The present
  menu, up to four, stays.

**A, a menu whose size follows the doubt (conformal prediction sets; tried, not shipped).** Instead of up to four
types, offer the smallest prefix of the likeliest types that holds a mass `q`, and ask when it has two or more.
`q` is the `ceil((n + 1)(1 - alpha))`-th smallest probability above the real type on a calibration set
(`val1-h4`), which makes the real type fall in the set in at least `1 - alpha` of commits of the same kind. The
model does not write the list: a list written as text has no probabilities to calibrate, and there is no
training signal for a set (a commit has one label). Fixed before the numbers, on `eval-v1` (not used to
calibrate): for each v0.3 model, the alpha whose share asked is closest to the present menu asking below 0.6
must have a type right (the real type is in the set) at least 0.004 under the present one and a menu at least
0.3 lines shorter.

- **The guarantee holds on other repositories.** For every alpha from 0.04 to 0.30 the coverage on eval-v1 is
  at or above `1 - alpha` for both models (0.10: 0.928 and 0.948 against 0.90; 0.20: 0.800 and 0.860 against
  0.80).
- **The pre-registered criterion fails.** At 0.28 asked the 1.5B's sets reach a type right of 0.768, 0.056 under
  the present menu at 0.6 (0.824); the 0.5B, at 0.41 asked, 0.776 against 0.836. The menus are 1.6 to 1.7 lines shorter,
  but the type right is what was asked to hold.
- **Exploratory, at the same type right** (interpolated on eval-v1; lines a person reads per commit = share
  asked × lines of the menu):

| model | present: asks, lines per commit | sets at the same type right: asks, lines per commit |
|---|---|---|
| 1.5B v0.3, 0.788 | 0.160, 0.61 | 0.324, 0.68 (+12%) |
| 1.5B v0.3, 0.824 | 0.272, 1.03 | 0.410, 0.89 (−14%) |
| 1.5B v0.3, 0.880 | 0.412, 1.49 | 0.553, 1.30 (−13%) |
| 0.5B v0.3, 0.780 | 0.248, 0.97 | 0.420, 0.92 (−5%) |
| 0.5B v0.3, 0.836 | 0.388, 1.46 | 0.547, 1.23 (−16%) |
| 0.5B v0.3, 0.888 | 0.544, 2.00 | 0.712, 1.76 (−12%) |

  The sets read 5% to 16% fewer lines (one point is 12% more) but interrupt 14 to 18 points more of the commits,
  because a set of two asks for what the present rule treats as sure enough. Which is better depends on what an
  interruption costs against a line, which is not measured here, and 250 commits make a 10% difference noise.
- **Verdict.** Not built. What the sets give is a knob that means something (`model.ask_type_below` is a
  confidence, a coverage is a promise: "the right type is on the menu in 90% of the commits"), at the price of
  asking in 80% of the commits for it. If the knob is wanted, the next test is the person: how long a choice takes
  with two types and with four, and how many interruptions they accept.

### H14. Examples chosen by the files they touched teach the style better — no gain without training

**Claim.** The prompt shows the five most recent subjects of the repository as examples of its style. The commits
that touched the files being committed should say more about how this change is described (vocabulary, scope,
level of detail) than the last five commits, which may be about anything (retrieval-augmented generation, RACE
and CoRaCMG in the literature, with the diffs of the similar commits, which our data do not keep).

**Design.** `--examples similar|mixed` (`prompt` and `eval` only): for each staged path, a past commit scores 3
if it touched that path, 2 if a file of its directory, 1 if a file of its top-level directory; the best-scoring
commits (similar: up to 5, mixed: up to 3; ties to the more recent) are shown, completed with the most recent
ones to the same five examples, in the history's order, so the prompt keeps its size. The rest of the prompt
and the models are as in `compare-v04c`: the 1.5B and 0.5B v0.3, annotated, full rules, `vulkan`, 500 commits of
`data/val1-h4` (250 Conventional and 250 free), no retraining. Fixed before the numbers: worth it if, for both
models, `word_f1` rises by at least 0.015 with the 95% bootstrap interval of the paired difference
(`paired_f1.py`) above 0, `first_copied` and `ungrounded` rise by at most 0.01 and `type_match` falls by at most
0.01; if it passes, one read on `eval-v1`.

**Result** (arm minus control, the control being the traces of `compare-v04c`):

| model, arm | `word_f1` all [95%] | Conventional | free | `type_match` | `first_copied` (conv., free) | `ungrounded` (conv., free) |
|---|---|---|---|---|---|---|
| 1.5B, similar | +0.009 [−0.003, +0.021] | +0.019 [+0.001, +0.038] | −0.001 | −0.020 | 0, +0.008 | 0, +0.008 |
| 1.5B, mixed | +0.003 [−0.007, +0.012] | +0.003 | +0.002 | −0.012 | 0, +0.004 | +0.004, 0 |
| 0.5B, similar | +0.005 [−0.006, +0.015] | +0.005 | +0.004 | +0.004 | 0, 0 | 0, +0.004 |
| 0.5B, mixed | +0.006 [−0.002, +0.014] | +0.008 | +0.004 | −0.008 | 0, 0 | 0, +0.004 |

- **It fails.** No arm raises `word_f1` by 0.015 with an interval above 0 in both models: the 0.5B gains 0.005 to
  0.006 (the interval holds 0 for all and the Conventional set). The 1.5B with `similar` is the only arm that
  clears the bar, and only on the Conventional set (+0.019, interval just above 0), where it loses 0.020 of
  `type_match` (0.648 to 0.628, over the 0.01 allowed). The free set, where the history is least
  informative, does not move.
- **The type does not gain either**: `type_group` and `type_behaviour` move with `type_match` (1.5B, similar:
  −0.024 and −0.016), so the commits that touched the same files do not say what type this one is, which the
  probe of the types by file (H13) had shown.
- **No new failures**: `first_copied`, `ungrounded`, `fallback` and `first_try` stay within 0.01 (a hundredth is
  two or three commits of 250).
- **Limits.** The models were trained with the recent examples, so a prompt they were not trained on may
  understate the gain; and this is the cheap form of retrieval, which shows only the subjects of the similar
  commits. What RACE and CoRaCMG retrieve is the diff and the message, which needs history diffs (the data keep
  subjects and file lists) and a model trained to read them: that is the form not tested.
- **Verdict.** Not adopted; the product keeps the most recent examples. The option stays for evaluation, like
  `--scopes map`. If retrieval is tried again, it needs the diffs of the history in the data and a model trained
  on them, which is a project of its own (and of the prompt budget: 1024 tokens leave little room for a second
  diff).

### H15. Three ways to sharpen the type without training — a cascade between the two sizes holds the quality; the other two do not

**Where they come from.** The [Alien Space of Science](https://github.com/alejandrohdez00/alien-space-of-science)
(Artiles et al., [arXiv 2603.01092](https://arxiv.org/abs/2603.01092)) scores combinations of the ideas ("atoms") of
16,068 papers on LLMs by how coherent they are and how little the community explores them; we scored the 11,480
triplets of 42 atoms close to our problem with the project's released models (CPU, no LLM, with a script of ours that
is not kept: the combinations only suggested what to try, they are not a result), and searched [Lacuna](https://lacuna.tiptreesystems.com) (a read-only map of ML research,
its MCP) for generated hypotheses on ambiguous labels, noisy targets and calibration. Combinations that repeat what we
had done (conformal sets, calibration, DPO) came first; three were new, and cheap to try on the type distributions
collected for H13 (`type_confidence.py collect`, first token of the reply, 250 Conventional commits of `val1-h4`
to choose the weight and of `eval-v1` to read it, the v0.3 models, `vulkan`). The weights are chosen on `val1-h4`;
`eval-v1` is read once. `type_fusion.py` has the three.

**Criteria fixed before the numbers.** *Prior* and *fusion*: worth it if, for both models, `type_match` rises by at
least 0.016 (4 commits of 250) over weight 0, the negative log-likelihood does not worsen and the ECE rises by at
most 0.02. *Cascade*: the `tau` chosen on `val1-h4` as the one that makes the big model answer least while keeping
`type_match` within 0.008 and `word_f1` within 0.010 of the big model alone; worth it if on `eval-v1` the big model
answers at most half of the commits within the same margins.

**1. The repository's use of each type as a prior** (`p(t) * ((count_t + 1) / (n + K)) ** lam`, the counts of the
types in the history's subjects): not worth it. The 1.5B's best weight is 0 (`val1-h4`: 0.644, 0.632, 0.616, 0.624,
0.608, 0.600 for 0, 0.25, 0.5, 0.75, 1, 1.5): the model already reads the history. The 0.5B chooses 0.25 and gains
0.012 on `eval-v1` (0.668 to 0.680, +6/−3; NLL 1.030 to 1.019, ECE 0.046 to 0.036), under the 0.016.

**2. A product of experts with a plain classifier** (`p(t) * (clf(t) + 0.001) ** w`, the classifier of H13, 0.612 on
`val1-h4` and 0.628 on `eval-v1`): not worth it. On `eval-v1` the 1.5B (w = 1) falls from 0.692 to 0.680 (+8/−11),
NLL 0.958 to 1.039, ECE 0.059 to 0.131; the 0.5B (w = 0.75) rises from 0.668 to 0.676 (+12/−10), NLL 1.030 to 1.048,
ECE 0.046 to 0.090. The product is too sure of itself, and the choice on `val1-h4` does not carry over. The ranking of
the confidence rises (AUROC 0.738 to 0.781 and 0.723 to 0.751), within the noise of 250 commits, and is not what was
asked.

**3. A cascade, the 0.5B v0.3 answering when it is at least `tau` sure of its type and the 1.5B v0.3 otherwise**:
**passes**. `tau` = 0.5 on `val1-h4`. On `eval-v1`:

| | the big model answers | `type_match` | `word_f1` |
|---|---|---|---|
| 0.5B alone (tau 0) | 0 | 0.676 | 0.354 |
| cascade, tau 0.4 | 0.116 | 0.692 | 0.360 |
| **cascade, tau 0.5 (chosen)** | **0.264** | **0.692** | **0.366** |
| cascade, tau 0.6 | 0.400 | 0.692 | 0.370 |
| cascade, tau 0.7 | 0.552 | 0.696 | 0.369 |
| 1.5B alone | 1.000 | 0.696 | 0.366 |

- **The loss is one commit and the words are the same**: 0.004 under the 1.5B in `type_match` and no difference in
  `word_f1`, with the 1.5B answering 26% of the commits. With 250 commits a difference of 0.03 cannot be excluded,
  and `tau` = 0.5 is on the edge of the margin on `val1-h4` (0.644 against 0.652).
- **Measured on the machine** (`cascade_bench.py`: both servers loaded or started per message, the product's request
  and flags; `eval-v1`'s Conventional commits, tau = 0.5; Ryzen 5 5500, RTX 3050 `vulkan` for 250 commits and the `cpu`
  for the first 100; one round, so the differences of a few hundredths of a second are not claimed). The 1.5B answered
  26.4% of the commits on `vulkan` and 27.0% on the `cpu`, as the offline count:

| | `vulkan`, loaded | `vulkan`, started per message | `cpu`, loaded | `cpu`, started per message |
|---|---|---|---|---|
| 1.5B alone | 0.25 s | 2.37 s | 2.42 s | 2.93 s |
| 0.5B alone | 0.13 s | | 1.16 s | |
| cascade, single pass | **0.19 s (−23%)** | **1.96 s (−17%)**, p90 3.73 s against 2.45 s | **1.78 s (−27%)** | **2.32 s (−21%)** |
| cascade, probe then reply | 0.24 s (−2%) | | 2.50 s (+3%) | |

- **The single pass is the only form that pays**: the 0.5B writes the whole reply, asking for the probabilities of
  its tokens (free: 0.126 s with and without), and the 1.5B is called only when the first token is under 0.5. The
  probe-then-reply form, the way `model.ask_type` reads the confidence, reads the prompt twice (the product sends
  `cache_prompt: false`) and saves nothing.
- **On a GPU it does not matter**: with the model loaded a message takes 0.25 s and the cascade saves 0.06 s, which
  no one notices. Started per message it saves 0.4 s on average but makes the escalated commits slower
  (two loads), so the worst tenth gets worse. A first estimate, made from the "per message" times of
  `docs/measurements.md` (2.3 s and 1.2 s on `vulkan`), gave 21% and 24% less; those times are mostly the model's
  load (2.1 s of the 1.5B's 2.3 s), so the percentages came out alike but the seconds are not what a message takes.
- **On the CPU it saves the most in seconds**: 0.64 s of 2.42 s with the models loaded (the prompt is read once, by the
  0.5B, for 73% of the commits). The price is the 0.5B's memory beside the 1.5B's, and a second server.
- **Quality, read from the replies' first lines**: the cascade's type matches 0.684 against the 1.5B's 0.692 and the
  0.5B's 0.664 on `vulkan` (250 commits), as the offline 0.692 against 0.696; on the `cpu` subset (100) the three tie at
  0.660.
- **Verdict.** Not built. On the devices most people use (a GPU, or the NPU, where it cannot be done) there is
  nothing to gain; on the `cpu` it is a 27% cut with the type held, and worth building only if that device matters
  enough to keep two models loaded. If it is built it must be the single pass, with the confidence taken from the
  first token's probabilities of the reply, and `tau` confirmed on a third set.

### H16. Naming what changed in the prompt helps the description — no gain without training

**Claim.** The prompt cuts the diff to its budget, and what is cut can hold the names the message should use. The
paper [Brevity is the Soul of Wit](https://lacuna.tiptreesystems.com/work/brevity-is-the-soul-of-wit-condensing-code)
condenses a change into a template with the summarised changes, the developers' comments and the key identifiers, and
fine-tunes on it (found with Lacuna, H15). A line with the names the change declares, and one with the comments it
adds, read from every file and not only the hunks that fit, should give the model what the budget took.

**Design.** `--names on` (`prompt` and `eval` only): `Names touched:` lists up to 8 functions, classes, structs and
the like that the hunk headers name or the changed lines declare, and `Comments added:` up to 2 comments of the added
lines (70 characters), from every source, config and test file; lockfiles, generated files, documents and binaries
give none. The lines come between the file list and the diff and are extra: the diff keeps its room. The models and
the rest of the prompt are as in `compare-v04c` (the 1.5B and 0.5B v0.3, annotated, full rules, `vulkan`, the 500
commits of `data/val1-h4`), without retraining. Fixed before the numbers: worth it if, for both models, `word_f1`
(all 500 commits) rises by at least 0.015 with the 95% interval of the paired difference (`paired_f1.py`) above 0,
`ungrounded`, `unsupported` and `first_copied` rise by at most 0.01 and `type_match` falls by at most 0.01; if it
passes, one read on `eval-v1`.

**Result** (arm minus control):

| model | `word_f1` all [95%] | Conventional | free | `type_match` | `ungrounded` | `unsupported` | `first_copied` |
|---|---|---|---|---|---|---|---|
| 1.5B v0.3 | +0.002 [−0.005, +0.010] | +0.003 | +0.002 | **−0.016** | −0.004 | −0.002 | 0 |
| 0.5B v0.3 | +0.002 [−0.005, +0.008] | +0.002 | +0.002 | +0.004 | 0 | +0.002 | 0 |

- **It fails.** The words do not move (+0.002 in both models, the interval holds 0), and the 1.5B loses 0.016 of
  `type_match`, over the 0.010 allowed (`type_group` −0.016 and `type_behaviour` −0.012 with it). Nothing worsens.
  `eval-v1` is not read.
- **The line was there and was read.** 278 of the 499 prompts that reach the model (56%) got a `Names touched` line
  and 85 a `Comments added` line. A subject uses one of the names listed in 27% of those with the line against 20% for
  the control (1.5B), and 32% against 27% (0.5B): the models take the names, and some take the comment whole
  (`fix: strip certain args from DOCKER_ARGS` after the comment "strip certain args from DOCKER_ARGS").
- **The authors do not use them as much.** The real subject uses one of the listed names in 44 of the 278 (16%). The
  models now name things more often than the authors do, so the words they add are not the words the author wrote.
  Even on those 278 commits `word_f1` moves +0.002 [−0.012, +0.016] (1.5B) and +0.003 [−0.008, +0.016] (0.5B); the
  headroom is small and the line does not reach it.
- **Limits.** The models were not trained with the line, which is how the paper used it; training with it is the form
  not tested, and given the 16% a large gain is not to be expected. The extraction is plain text (declaration
  keywords, hunk headers, comment markers), not a parser, and is English-centred in its comments.
- **Verdict.** Not adopted; the product keeps the prompt as it is. The option stays for evaluation.

## Ideas not tried yet

- An LLM judge for faithfulness, validated against a sample rated by
  hand; `unsupported` and `ungrounded` in `sc-training score` are the cheap checks.
- A body in the target when the real commit has one.
- A second epoch instead of more data (H3's curve flattens at about 20k).
- Portuguese and gitmoji targets: v0.1 is English, and eval-v1 has neither.

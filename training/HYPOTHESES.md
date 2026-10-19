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
| H1 (1.5B) | pending | | | | | |
| H2 (0.5B) | pending | | | | | |

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

### H1. A LoRA on 10k examples fixes the type bias — running

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

### H2. The 0.5B, trained the same way, gets close to the trained 1.5B — running

Same data and recipe as H1, on Kaggle's second T4. The 0.5B reads the
prompt about 1.6× faster on the CPU (prefill 1916 against 2998 ms in
docs/training.md); untrained it was not faster end to end only because it
copied history subjects and ran to the 128-token limit. Load time barely
differs (311 against 369 ms). If it gets within noise of H1 on type_match
and word_f1, with first_try ≥ 0.98, it is the better default for slow
CPUs; it still has no OpenVINO export for the NPU and Intel GPU.

## Ideas not tried yet

- Train without the rules in the system prompt (shorter prefill); needs a
  `PromptOptions` switch in smart-commit.
- More data / a second epoch, if H1 improves and the loss keeps falling.
- A body in the target when the real commit has one.

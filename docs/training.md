# Training a commit-message model

Work in progress on the `training` branch. This page is the hand-off for
whoever trains the model (a person, or an agent on the training machine): what
was measured, what was decided, and what is left to do. Read
[AGENTS.md](../AGENTS.md) first; its rules apply here too.

## Goal

Fine-tune a small model for smart-commit's exact task and prompt format,
publish it on Hugging Face, and add it to the recommended models
(`src/models.rs`), pinned by revision and SHA-256 like the others.

Two expected gains:

- **Quality.** The current default, Qwen2.5-Coder-1.5B-Instruct, labels
  almost every change `fix` (6 of 6 in the measurements below, where most
  commits were `feat`), sometimes uses a file path as the scope, and needs a
  retry when the reply does not follow the convention.
- **Speed, indirectly.** A model trained on the format does not need the
  ~250 tokens of rules in the system prompt, and a fine-tuned
  Qwen2.5-Coder-0.5B may become usable (untrained, it copies the history
  examples instead of describing the diff; see below).

## Decisions already made

- **Training is not bit-for-bit reproducible** (GPU kernels are not
  deterministic), and that is accepted. The trained weights are treated like
  the upstream GGUF files: a pinned input, published together with the exact
  dataset and recipe. `docs/reproducibility.md` must say so when the model is
  added. Inference stays greedy and reproducible.
- **Training happens on the machine with the RTX 3050** (Ryzen 5 5500), not
  on the development laptop. Check the VRAM with `nvidia-smi` (the desktop
  3050 has 8 GB; QLoRA on a 1.5B model fits).
- **Base model: Qwen2.5-Coder-1.5B-Instruct first**, then the 0.5B with the
  same pipeline. A LoRA on the 1.5B can be applied by llama.cpp (`--lora`)
  on top of the GGUF that is already pinned.
- **The prompt the model sees in training must be byte-identical to what
  smart-commit builds at run time.** Never write a second prompt builder in
  Python; generate the inputs with the Rust code (see
  [Dataset](#dataset)).
- **The fine-tunes are published on Hugging Face by the project**, as
  `smart-commit-<size>-v<version>` (AGENTS.md, "Where models come from"); other
  publishers' models are still never re-hosted.
- **Evaluation comes before training.** Without a held-out set and metrics
  there is no way to tell whether a fine-tune, a smaller prompt budget or
  another model is better.

## Where the time goes today

Prefill (reading the prompt) dominates: on the CPU it is about 90% of the
time per message. Anything that does not shorten the prompt or speed up
prefill barely matters, because replies are ~12 tokens.

Measured on an Intel Core Ultra 5 225H (14 cores), llama.cpp b11381,
Qwen2.5-Coder-1.5B-Instruct Q4_K_M, means over 6 real prompts of 581–853
tokens, in milliseconds per message (load + one generation):

| Configuration | Load | Prefill | Decode | Total |
|---|---|---|---|---|
| `cpu` as shipped (`--no-repack --fit off`) | 369 | 6992 | 512 | **7873** |
| + `-tb 14` | 369 | 3369 | 517 | **4255** |
| + `-tb 14 -b 1024 -ub 1024` | 389 | 2998 | 518 | **3905** |
| + the above, `model.max_prompt_tokens = 512` | 369 | 1664 | 577 | **2610** |
| `vulkan` as shipped (Intel Arc iGPU) | 658 | 1095 | 735 | **2487** |
| `vulkan`, `model.max_prompt_tokens = 512` | 650 | 534 | 739 | **1923** |
| 0.5B, `cpu -tb 14 -b 1024 -ub 1024` | 311 | 1916 | 1966 | **4193** |

End to end with `smart-commit --device ...` on three of the same commits:
`intel-gpu` (OpenVINO) about 2.0 s, `npu` about 6.3 s.

Findings:

- `llama-server` is started without `--threads`/`--threads-batch`, so it uses
  its own default, which on this hybrid CPU used 2 threads. Setting
  `--threads-batch` to the core count halves prefill and gave **identical
  replies** on all 6 prompts. Changing `--threads` (decode) changed the
  replies and made decode slower, so leave it alone. This is also a
  reproducibility issue: thread counts are library defaults today.
- `smart-commit bench` uses the trace fixtures, whose prompts are ~250
  tokens, so it underestimates prefill: it reports ~2.6 s per message for
  `cpu` on this machine, against ~7.9 s with real prompts. A large fixture
  is needed before `auto` can pick devices well.
- Prefill scales about linearly with prompt tokens: halving the budget
  halves it, at a cost in quality (with 512 some scopes became file paths,
  like `fix(src/bench.rs)`).
- The untrained 0.5B is not faster end to end: it copied history subjects
  verbatim as its answer and ran to the 128-token limit on 4 of 6 prompts.
- No gain: flash attention (`-fa on`), weight repacking (slower; confirms
  fast load), speculative decoding (replies are too short).

### Reproducing the measurements

```bash
# Prompts from real commits of this repository, rebuilt in a scratch clone:
git clone -q . /tmp/sc && cd /tmp/sc
mkdir -p /tmp/prompts
for c in 8bc78c6 3df6503 badc90d de0dfed c4687aa 8314005; do
  git checkout -q -f "$c^" && git cherry-pick -n "$c" >/dev/null
  smart-commit prompt > "/tmp/prompts/$c.txt"
  git reset -q --hard
done
# The 512 budget: the same loop with XDG_CONFIG_HOME pointing at a config
# with `[model] max_prompt_tokens = 512`.

M=~/.local/share/smart-commit/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf
CPU="--n-gpu-layers 0 --device none --no-repack --fit off"
python3 -I training/time-llama-server.py "$M" /tmp/prompts warmup $CPU >/dev/null
python3 -I training/time-llama-server.py "$M" /tmp/prompts "cpu" $CPU
python3 -I training/time-llama-server.py "$M" /tmp/prompts "cpu tb" $CPU -tb "$(nproc)" -b 1024 -ub 1024
python3 -I training/time-llama-server.py "$M" /tmp/prompts "vulkan" \
  --n-gpu-layers 999 --device Vulkan0 --no-repack --fit off
```

The prompts depend on the history at each commit (older commits were in
Portuguese, so those prompts ask for Portuguese).

## Options considered

| Option | Verdict | Why |
|---|---|---|
| **Fine-tuning** (LoRA/QLoRA) | **do it** | fixes quality where the base model fails, shortens the prompt, may make 0.5B usable |
| Other runtimes (colibri, vLLM, SGLang, Ollama, MLX…) | no | colibri targets huge MoE models streamed from disk; server engines load slowly and need CUDA; Ollama is llama.cpp with less control |
| [FastFlowLM](https://github.com/FastFlowLM/FastFlowLM) | later, separate work | the only way to AMD XDNA 2 NPUs on Linux; TCP only, own model format, determinism undocumented |
| [ik_llama.cpp](https://github.com/ikawrakow/ik_llama.cpp) | try later | faster CPU prefill, same server interface |
| [LongCodeZip](https://arxiv.org/abs/2510.00446) | idea only | ranks code by perplexity, which costs a prefill over the whole input (~2 s with the 0.5B on CPU), more than it saves under a 1024-token budget; borrow the idea of cutting by hunk/function instead of by line, deterministically |
| [llm.npu](https://arxiv.org/abs/2407.05858) | no | Qualcomm mobile NPUs; its chunked prefill is already what `model.npu_prefill_chunk` does |
| Encoder-decoder ([Return of the Encoder](https://arxiv.org/abs/2501.16273); CodeT5+, T5Gemma) | only after fine-tuning works | the gain comes from a smaller model reading the input, which needs task fine-tuning anyway; `llama-server` and OpenVINO GenAI's `LLMPipeline` serve decoder-only models, so it would be a new backend |
| Grammar-constrained decoding (GBNF in `llama-server`) | do it, separately | removes the convention retry, which doubles the time when it happens |

## Existing fine-tunes to compare against

- [marzoukbaig14/committed-gguf](https://huggingface.co/marzoukbaig14/committed-gguf):
  Qwen3-1.7B, QLoRA, Q4_K_M GGUF (1.1 GB), Apache-2.0. Trained on ~58k
  filtered CommitChronicle commits, single-file diffs, English, decoded with
  a GBNF grammar. Its card reports type accuracy 0.131 → 0.637 (LLM judge,
  442 examples) and that the base model answered `feat` 95.5% of the time.
  It does not know smart-commit's prompt (history examples, language,
  scopes), so it is a baseline, not a drop-in.
- [Elib27/qwen2.5-coder-1.5b-commit-msg-lora](https://huggingface.co/Elib27/qwen2.5-coder-1.5b-commit-msg-lora)
  (and [0.5B](https://huggingface.co/Elib27/qwen2.5-coder-0.5b-commit-msg-lora)):
  PEFT LoRA (r=16, alpha=32) on our exact base model, ~11k pairs. No license
  stated, so it cannot be redistributed, only compared.
- [CommitLLM](https://arxiv.org/abs/2607.17532) (Mistral-7B QLoRA): reports
  that post-processing contributed more than fine-tuning, which matches
  smart-commit's sanitizer approach.

## Dataset

### Source

[JetBrains-Research/commit-chronicle](https://huggingface.co/datasets/JetBrains-Research/commit-chronicle):
10.7M commits from permissively licensed repositories, 20 programming
languages; splits `train` (7.66M), `validation` (1.55M), `test` (1.49M);
configs `default`, `subset_cmg` (204k), `subset_llm` (4.03k). Fields:
`author`, `date` (`DD.MM.YYYY HH:MM:SS`), `timezone`, `hash`, `message`
(cleaned), `original_message`, `mods` (list of `change_type` `ADD`/`MODIFY`/
`DELETE`/…, `old_path`, `new_path`, `diff` with `@@` hunks), `language`,
`license`, `repo`.

**Open question, blocking publication:** the dataset's license is listed as
"other". Read its full terms before publishing weights trained on it.

Portuguese is scarce there. Options: repositories with Portuguese history
collected separately, or labels written by a larger model (check that
model's terms allow training on its outputs). Start with English.

### From a commit to a training example

smart-commit's traces already record everything the prompt is built from,
as `trace::Inputs` (`src/trace.rs`):

```json
{
  "branch": "main",
  "subjects": ["feat(git): collect the history", "fix(diff): parse renames"],
  "staged": [{"status": "modified", "path": "src/diff.rs", "old_path": null}],
  "files": [{"path": "src/diff.rs", "old_path": null, "lines": [3, 1],
             "hunks": "@@ -10,2 +10,4 @@\n..."}]
}
```

- `subjects`: the previous commits of the same repository, newest first (the
  dataset has `repo` and `date`; sort and take the 30 before the commit, as
  `style.history_depth` does). This is what teaches the model to follow the
  repository's style.
- `staged[].status`: `added`, `modified`, `deleted`, `renamed`, `copied`,
  `type_changed` (snake_case of `git::ChangeStatus`).
- `files[].lines`: `[added, deleted]`, `null` for binary files;
  `files[].hunks`: the patch from the first `@@` on.
- `branch`: unknown in the dataset; use `null` or `"main"` consistently.
- Target: the commit's subject (and body, if kept), cleaned the way the
  sanitizer would accept it.

The prompt is then built **by smart-commit**, not by the Python code:
`smart-commit prompt --inputs FILE` reads JSON Lines of `Inputs` and writes
JSON Lines of `{"system", "user"}`, with `prompt::build` and
`Inputs::context`, as `replay` does ([usage](usage.md)). Run it with an empty
`XDG_CONFIG_HOME` so the user's config does not leak into the dataset, and
record the `PromptOptions` used.

The diff fitting (`src/diff.rs`) uses one line of context (`-U1`), while
CommitChronicle diffs have more. Either trim the context to one line when
converting, or accept the difference; trimming keeps training closer to run
time.

### Filtering

- Keep commits whose subject is a valid message for the repository's
  convention (Conventional Commits, gitmoji or free), 8 to 72 columns.
- Drop merges, reverts, `fixup!`/`squash!`, WIP, release bumps, bot authors,
  and messages that only reference an issue.
- Balance the `type` distribution, or the model learns the dataset's bias
  instead of the base model's.
- Split by **repository**, never by commit, so the evaluation measures
  unseen repositories.

## Evaluation

Build it first, and use it for every later decision (fine-tune, 0.5B vs 1.5B,
prompt budget, grammar). It is item 1 of the [roadmap](roadmap.md)
(`smart-commit eval`), which already lists the metrics, including copied
history examples; build that rather than a separate Python evaluator, so the
same numbers judge every model:

- A held-out set of repositories, a few hundred commits, prompts built by
  smart-commit.
- Automatic metrics: the sanitizer accepts the reply without retry; the type
  matches the real one; the language matches; the subject fits 72 columns;
  the scope is one of the repository's scopes.
- Optional: an LLM judge for faithfulness to the diff, validated against a
  sample rated by hand.
- Always report the base model's scores on the same set.

## Training

- LoRA with `transformers` and `peft`, in a Python environment with every
  version pinned (`training/uv.lock`). On the 3050 the 1.5B fits in bf16
  (4.8 GiB peak), which was faster than QLoRA (0.69 against 0.85 s per
  example); `train.py --quant 4bit` keeps QLoRA for larger models.
- Format each example with the model's own chat template (system + user →
  assistant), and compute the loss on the assistant reply only.
- Sequence length: the prompt budget (1024) plus the reply (128).
- Record everything needed to repeat the run: dataset revision and filters,
  base model revision, library versions, hyperparameters, seed, the GPU and
  driver.

## Export and publication

What is published is one model per size smart-commit offers, named
`smart-commit-<size>-v<version>` (`smart-commit-1.5b-v0.1`,
`smart-commit-0.5b-v0.1`), not the experiment runs: those stay in
`training/runs/` and their numbers in HYPOTHESES.md. A later training is a new version, never a replaced file.

1. Merge the adapter and quantize to Q4_K_M with `export_gguf.py` (the
   llama.cpp tag smart-commit pins, b11381), and judge it against the
   control as H0 explains. The merged GGUF runs with no change to
   smart-commit (`SMART_COMMIT_GGUF`); the adapter alone
   (`export_lora.py`, `llama-server --lora`) is for evaluation only.
2. For the OpenVINO devices (NPU, Intel GPU), users' machines export the
   published safetensors through `scripts/export/`, following AGENTS.md's
   model export rules (two identical exports, `MANIFEST.sha256`, the
   **Model** workflow).
3. Upload both to the project's Hugging Face account,
   [LucasBRT](https://huggingface.co/LucasBRT), one repository per model and
   version (`LucasBRT/smart-commit-1.5b-v0.1`): the merged safetensors, the
   Q4_K_M GGUF and a model card with the base model (`base_model:` in its
   metadata), the dataset, the prompt format (smart-commit's), the
   evaluation against the base model, and the limitations (languages).
   The weights are Apache-2.0, like the base model. CommitChronicle has no
   single licence (`license: other`): it gathers commits from repositories
   under MIT, Apache-2.0 and BSD-3-Clause and asks that uses follow their
   terms, attribution included. The card therefore cites CommitChronicle
   (revision `5fd076e`), says the training commits come from repositories
   under those licences, and points to the dataset's per-commit provenance
   (its `repo` and `license` fields).
4. Add it to `REGISTRY` in `src/models.rs` with the repository, the commit
   revision (never a branch), the size and the SHA-256, initially as
   `Status::Experimental`; update the pages that say models come only from
   their publishers (`docs/models.md`, `docs/usage.md`, README) and
   `docs/reproducibility.md`.

## Next steps, in order

1. ~~`smart-commit prompt --inputs` (Rust, with tests).~~ Done.
2. ~~Dataset builder: CommitChronicle → `Inputs` → prompts → JSON Lines with
   targets, split by repository.~~ Done: [training/](../training/README.md).
3. ~~Evaluation, and the base 1.5B and 0.5B scores.~~ Done: `smart-commit
   eval` and `training/score.py`; scores in
   [training/HYPOTHESES.md](../training/HYPOTHESES.md).
4. ~~LoRA on the 1.5B; compare; then the 0.5B.~~ Done: H1 to H3 in
   HYPOTHESES.md.
5. ~~Export, publish, add to the registry.~~ Done for v0.1: the c40k runs
   of H3, published as
   [smart-commit-1.5b-v0.1](https://huggingface.co/LucasBRT/smart-commit-1.5b-v0.1)
   and [smart-commit-0.5b-v0.1](https://huggingface.co/LucasBRT/smart-commit-0.5b-v0.1),
   experimental models in `src/models.rs`.
6. The next version: H5 (young repositories) and H6 (the rules in the
   weights) in one bundle, on about 20k examples.

Independent of training, on `main`: `--threads-batch` is passed explicitly
(one thread per physical core, [devices](devices.md#threads-on-the-cpu));
a model server that stays up, KV caching of the fixed prompt prefix and a
GBNF grammar for the conventions are in the [roadmap](roadmap.md). Still
to do: a large trace fixture for `bench`, whose ~250-token prompts
underestimate prefill.

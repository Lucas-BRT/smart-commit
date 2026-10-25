# Training a commit-message model

Work in progress, under `training/`. This page is the hand-off for
whoever trains the model (a person, or an agent on the training machine): what
was measured, what was decided, and what is left to do. Read
[AGENTS.md](../AGENTS.md) first; its rules apply here too.

## Goal

Fine-tune a small model for smart-commit's exact task and prompt format,
publish it on Hugging Face, and add it to the recommended models
(`src/models/registry.rs`), pinned by revision and SHA-256 like the others.

Two expected gains:

- **Quality.** The current default, Qwen2.5-Coder-1.5B-Instruct, labels
  86.8% of the conventional commits of eval-v1 `fix`, where 41% really are
  ([HYPOTHESES.md](../training/HYPOTHESES.md)), sometimes uses a file path
  as the scope, and needs a retry when the reply does not follow the
  convention.
- **Speed, indirectly.** A model trained on the format does not need the
  ~105 tokens of rules in the system prompt (training H6), and a fine-tuned
  Qwen2.5-Coder-0.5B may become usable (untrained, it copies the history
  examples instead of describing the diff; see below).

## Decisions already made

- **Training is not bit-for-bit reproducible** (GPU kernels are not
  deterministic), and that is accepted. The trained weights are treated like
  the upstream GGUF files: a pinned input, published together with the exact
  dataset and recipe. `docs/reproducibility.md` must say so when the model is
  added. Inference stays greedy and reproducible.
- **Training runs on Kaggle** (`kaggle_train.ipynb`, T4 GPUs without bf16,
  so QLoRA: a 4-bit NF4 base with fp16 compute). The first local run, on the
  RTX 3050 (Ryzen 5 5500), stalled when the machine suspended; that machine
  builds the data and runs the evaluations.
- **Base models: Qwen2.5-Coder-1.5B-Instruct and 0.5B**, with the same
  pipeline. What is published is the merged model; an adapter applied with
  llama.cpp's `--lora` on the upstream GGUF is for evaluation only.
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
time per message, and replies are ~12 tokens, so anything that does not
shorten the prompt or speed up prefill barely matters. The measurements
(configurations, findings, how to repeat them) are in
[measurements](measurements.md#where-the-time-goes-on-the-cpu-and-vulkan).

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

Its licence is listed as "other": it gathers commits from repositories
under MIT, Apache-2.0 and BSD-3-Clause; how the fine-tunes credit it is in
[Export and publication](#export-and-publication).

Portuguese is scarce there, and v0.1 was trained on English only. Options:
repositories with Portuguese history collected separately, or labels
written by a larger model (check that model's terms allow training on its
outputs).

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

The diff fitting (`src/diff/`) uses one line of context (`-U1`), while
CommitChronicle diffs have more. Either trim the context to one line when
converting, or accept the difference; trimming keeps training closer to run
time.

### Filtering

- Keep commits whose subject is a valid message for the repository's
  convention (Conventional Commits, gitmoji or free), 8 to 72 columns.
- Drop merges, reverts, `fixup!`/`squash!`, WIP, release bumps, bot authors,
  and messages that only reference an issue. Dropping release bumps means
  the models never see one: v0.1 answered the 0.5.0 bump with `feat(model)`
  (fixture `conventional-en-release`, H7 in HYPOTHESES.md).
- Balance the `type` distribution, or the model learns the dataset's bias
  instead of the base model's.
- Split by **repository**, never by commit, so the evaluation measures
  unseen repositories.

## Evaluation

Every decision (fine-tune, 0.5B against 1.5B, prompt, data) is judged with
`smart-commit eval`, which runs the whole pipeline, and
`training/sc-training score`, so the same numbers judge every model:

- **eval-v1**: 500 commits of CommitChronicle's test split, from 70
  repositories never seen in training, 250 Conventional and 250 free;
  **eval-short**: 197 commits of young repositories (1–5 history subjects).
  How to rebuild both is in [training/README.md](../training/README.md).
- **Scenarios**: hand-written cases of git operations on two synthetic
  repositories (Python in Portuguese with free style, TypeScript with
  Conventional Commits), by level of complexity from a typo to a merge in
  progress, each with its target; `sc-training score` reports them per level and
  checks the words a right subject must have (`expected`). Each case is in
  the train or the eval split, never both; those whose target depends on
  what the inputs do not record (merge, revert, file modes) are eval only.
  See [training/scenarios/README.md](../training/scenarios/README.md).
- Metrics: first_try, fallback, copied, type_match, scope_known,
  scope_fits, word_f1, and two cheap faithfulness checks, `unsupported`
  (specifics found nowhere in the input) and `ungrounded` (a description
  with no word from the diff); `sc-training score`'s docstring defines them.
- Always report the base model's scores on the same set and binary.
- Two versions of a model or a prompt answer the same cases: compare them
  case by case with `sc-training score EVAL_DIR NEW.jsonl --against OLD.jsonl`,
  which counts the cases each gets right that the other does not and gives
  McNemar's exact p. A change of 1–3 points on eval-v1 is often a handful
  of cases either way.
- Not covered yet: gitmoji, Portuguese beyond the scenarios, and an LLM
  judge for faithfulness validated against a sample rated by hand.

## Training

- LoRA with `transformers` and `peft`, in a Python environment with every
  version pinned (`training/uv.lock`). On Kaggle's T4, QLoRA
  (`train.py --quant 4bit`); on the 3050 the 1.5B also fits in bf16
  (4.8 GiB peak, 0.69 against 0.85 s per example for QLoRA).
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
2. For the OpenVINO devices (NPU, Intel GPU), users' machines are to
   export the published safetensors through `scripts/export/`, following
   AGENTS.md's model export rules (two identical exports, `MANIFEST.sha256`,
   the **Model** workflow). Not done for v0.1, which runs on `vulkan` and
   `cpu` only.
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
4. Add it to `REGISTRY` in `src/models/registry.rs` with the repository, the commit
   revision (never a branch), the size and the SHA-256, initially as
   `Status::Experimental`; update the pages that say models come only from
   their publishers (`docs/models.md`, `docs/usage.md`, README) and
   `docs/reproducibility.md`.

## Next steps, in order

1. ~~`smart-commit prompt --inputs` (Rust, with tests).~~ Done.
2. ~~Dataset builder: CommitChronicle → `Inputs` → prompts → JSON Lines with
   targets, split by repository.~~ Done: [training/](../training/README.md).
3. ~~Evaluation, and the base 1.5B and 0.5B scores.~~ Done: `smart-commit
   eval` and `training/sc-training score`; scores in
   [training/HYPOTHESES.md](../training/HYPOTHESES.md).
4. ~~LoRA on the 1.5B; compare; then the 0.5B.~~ Done: H1 to H3 in
   HYPOTHESES.md.
5. ~~Export, publish, add to the registry.~~ Done for v0.1: the c40k runs
   of H3, published as
   [smart-commit-1.5b-v0.1](https://huggingface.co/LucasBRT/smart-commit-1.5b-v0.1)
   and [smart-commit-0.5b-v0.1](https://huggingface.co/LucasBRT/smart-commit-0.5b-v0.1),
   experimental models in `src/models/registry.rs`.
6. ~~v0.2: H5 (young repositories), H6 (the rules in the weights), H7's
   release bumps and no type cap (H8) in one bundle of about 20k examples
   (b3).~~ Done: `smart-commit-0.5b-v0.2` and `smart-commit-1.5b-v0.2`,
   trained and evaluated, on Hugging Face and private (results in
   [HYPOTHESES.md](../training/HYPOTHESES.md), v0.2). The scope map (H4b)
   was not in the bundle.
7. The next version: H4b, and more data for the 1.5B, which leans on the
   rules more than the 0.5B. What to publish, register and make the default
   is in the [roadmap](roadmap.md).

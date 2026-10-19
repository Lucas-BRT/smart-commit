# Roadmap

## Done

- Repository style detection, diff compaction, prompt and sanitization.
- CLI with retry and fallback, traces and `replay`.
- A retry when the reply repeats a history subject, and lockfile-only
  changes written from the file list without loading the model.
- `prepare-commit-msg` hook for any git client.
- OpenVINO GenAI backend on the NPU, the Intel GPU and the CPU, with NPU
  tuning (chunked prefill, `BEST_PERF`).
- llama.cpp backend with Vulkan for AMD, NVIDIA and Intel GPUs, and on the
  CPU, so machines without Intel devices need no OpenVINO.
- `auto` device selection, `bench` and `device`.
- Recommended models downloaded from Hugging Face, pinned and checked
  (`smart-commit model`), or any GGUF file from disk.
- The first fine-tunes, `smart-commit-1.5b-v0.1` and
  `smart-commit-0.5b-v0.1`, as experimental models.
- Fast load for llama.cpp (`--no-repack --fit off`), turned on per device
  by the benchmark, and `--threads-batch` passed explicitly.
- Global and per-repository configuration.
- Installer, guided setup that lists the hardware first and installs only
  what each vendor's CPU, GPU and NPU need, reproducible model export and
  release packaging, automatic releases.
- `smart-commit eval` and `prompt --inputs`, which run the pipeline on
  recorded inputs; on the `training` branch, the eval-v1 and eval-short sets
  built with them, and the scope map in the prompt.

## Next

Highest priority first: what users hit now and what unblocks other items,
then the largest gains for their cost.

| # | Item | Why |
|---|---|---|
| 1 | **Setup survives an invalid setting** | Found in real use: a `model.name` the installed release does not know (written by a newer build) made the installer's `smart-commit setup` stop with an error, though setup is the tool that should fix it. Warn and offer the choice of model instead; a regression test with the unknown name. |
| 2 | **Fine-tune v0.2** | The next `smart-commit-<size>-v0.2`: the rules in the weights (training H6, a `PromptOptions` switch that follows the model in `models::REGISTRY`), young repositories (H5: the 0.5B falls back on 9% of them, the 1.5B on 3%) and the scope map (H4b), in one bundle of about 20k examples, where the v0.1 learning curve flattens. H6 is estimated to read typical prompts 40–60% faster on the CPU, to be measured. See `docs/training.md` on the `training` branch. |
| 3 | **A model server that stays up** | Load is paid at every commit: 0.3–0.6 s on the CPU, about 2.2 s on Vulkan (weights to the GPU, shaders compiled). Keeping `llama-server` running after a message, and stopping it after some idle minutes (say 10), makes every later commit pay only prefill and decoding. It also gives prefix caching for free: llama-server keeps the KV cache of the prompt's unchanged beginning in memory. Costs: 0.5–1.2 GB of RAM or VRAM while it is up, and its lifecycle (restart when the settings or the model change, two commits at once, a stale socket). The commit nudges would need a loaded model at hand too. Check that replies stay identical with the cache on, against the trace fixtures and `eval`, before turning it on: a restored cache can change float results. |
| 4 | **A large prompt for `bench`** | `bench` runs the trace fixtures, whose prompts are about 250 tokens: on the Core Ultra 5 225H it reports ~2.6 s per message on `cpu` against ~7.9 s with real prompts, so `auto` can pick the wrong device where prefill dominates. A fixture with a prompt near the 1024-token budget fixes the estimate. |
| 5 | **A fine-tune as the default** | On eval-v1 the v0.1 fine-tunes pick the real type 64% of the time against 46% for the default `qwen2.5-coder-1.5b`. Making one `validated` and the default (on `vulkan` and `cpu`, which need no OpenVINO export) needs the fixture prompts and the hardware tests; after v0.2's evaluation. |
| 6 | **Fine-tunes and more models on the OpenVINO devices** | The NPU and the Intel GPU run only `qwen2.5-coder-1.5b`, the one model with a pinned export. Export the fine-tunes' published safetensors, and other [recommended models](models.md), with the pinned pipeline: two identical exports, `MANIFEST.sha256` and the **Model** workflow for each. |
| 7 | **Evaluation coverage** | eval-v1 and eval-short are English, Conventional or free. Add gitmoji and Portuguese cases from permissively licensed repositories, pinned by commit, and an LLM judge for faithfulness with a pinned prompt, validated against a sample rated by hand (`score.py` has only cheap checks). |
| 8 | **GBNF grammar for the conventions** | A grammar in the request makes `type(scope): description` (or a gitmoji) the only possible shape, so the format retry, which doubles the time when it happens, goes away for every model. Less pressing with the fine-tunes, whose first reply passes 95–98% of the time on eval-v1 (91–93% for the base 1.5B). |
| 9 | **Golden replies per device** | `tests/fixtures/golden/<model>/<device>.json`, written only by a script; the hardware tests would flag when a driver or runtime update changes replies. |
| 10 | **Commit nudges** (`smart-commit status`, `watch`, `lsp`) | Work that starts as one small feature and drifts into tangents ends as one bloated commit: hard to review, and hard for the model to describe. One engine compares the working tree and the index with `HEAD` and says when it is time to commit; several fronts use it. Deterministic signals first, cheap enough to run all the time: diff size, how many areas are touched (top-level directories, modules, code / docs / config / lockfiles), and **drift from the focus** (the first files edited, or one declared with `smart-commit focus "..."` or taken from the branch). Then, debounced and only when the diff changed enough, the model's "one purpose or several?" judgement, once the training branch has one. Fronts in order: `status`, one short line for shell prompts (starship, tmux); `watch`, desktop notifications; `lsp`, a stdio language server with a message and code actions ("commit the focus files now", "move this tangent to a branch or stash"). Nudge, do not nag: rate limits, snooze, off per repository, and always a concrete action rather than a bare warning. Thresholds in `config::KEYS`; the clock injectable so tests stay deterministic; new dependencies (file watching, LSP) in commits of their own. |
| 11 | **Vulkan pipeline cache** | Part of the 2.2 s Vulkan load is compiling shaders. Measure whether llama.cpp's Vulkan backend can reuse a pipeline cache between runs, and how much it saves. |
| 12 | **Fewer history examples for trained models** | Up to 5 recent subjects (about 100 tokens) show the style; a model trained on the task may need fewer. Measure with `eval` (eval-v1 and the young-repository slice) before changing the default. |
| 13 | **Energy** | Joules per message on each device, from the package RAPL counter (it covers CPU, GPU and NPU). It needs root, either through a udev rule for the `render` group or by running the benchmark with `sudo`. With energy, `auto` could weigh more than time. |
| 14 | **CUDA and ROCm builds of llama.cpp** | Vulkan is enough everywhere; CUDA or ROCm may be faster on discrete GPUs. Measure first. |
| 15 | **Several GPUs** | `bench` measures one Vulkan device (`model.vulkan_device`); it could measure each. |
| 16 | **Other distributions** | The setup installs system packages with `apt` only and lists them elsewhere. |
| 17 | **OpenVINO 2026.4+** | When an NPU driver is validated with it. Newer runtimes also read the models the OpenVINO organisation publishes. |
| 18 | **The fixed prompt prefix cached on disk** (only if a server that stays up is not wanted) | Without a server that stays up, llama-server can save a slot's KV cache to a file and restore it (`--slot-save-path`), about 28 KB per token for the 1.5B model, 7 MB for 250 tokens. It needs the prompt to start with the part that never changes (the rules), then the repository's part. Same reproducibility check as 12. Little left to cache if the trained model drops the rules (training H6). |

## Model candidates

There is no public benchmark of small models for commit messages, and recent
work (CommitBench, CommitLLM) suggests that fitting the repository's context
matters more than model size. The choice goes through the evaluation
(`smart-commit eval`). Candidates of at most 5B parameters, which fit 15 GiB of RAM in
INT4:

| Model | Licence | On the NPU (OpenVINO 2026.3) | Notes |
|---|---|---|---|
| Qwen2.5-Coder-1.5B-Instruct (current) | Apache-2.0 | yes, validated | fast and code-focused; copies history examples when the diff says little (a retry rejects copies) |
| Qwen3-1.7B | Apache-2.0 | listed, with a known loading issue in 2026.3 | multilingual; needs `enable_thinking = false` |
| Qwen3-4B-Instruct-2507 | Apache-2.0 | Qwen3 family | no thinking mode; the strongest all-rounder of its size in comparisons; slower |
| LFM2.5-1.2B-Instruct | LFM 1.0 (revenue threshold for commercial use) | new in 2026.3 (export with `--backup-precision int8_sym`) | the smallest; built for edge devices |
| SmolLM3-3B | Apache-2.0 | new in 2026.3 | Portuguese among its 6 languages; weak on code in published numbers |
| Phi-4-mini-instruct (3.8B) | MIT | not listed (Phi-3.5 has an official example) | strong on code |
| Gemma 4 E2B-it | Apache-2.0 | supported since 2026.2 | multimodal, 5.1B parameters in total |
| Qwen3.5-0.8B / 2B | Apache-2.0 | no (CPU and GPU only) | multimodal, needs `VLMPipeline` and a newer runtime |

Excluded: Qwen2.5-Coder-3B (research licence, not open source) and
Llama 3.2 3B (gated, custom licence).

The OpenVINO organisation's ready-made conversions are not an option on
the current runtime: they target newer tooling (the LFM2.5 one crashed, and
Qwen3.5 is multimodal), and the small ones are INT4 asymmetric, which the
NPU does not take. Models are exported with the pinned toolchain instead.

The exploratory scripts behind these numbers are in `scripts/experiments/`.

## Sources

- [OpenVINO GenAI on NPU](https://docs.openvino.ai/2026/openvino-workflow-generative/inference-with-genai/inference-with-genai-on-npu.html)
- [OpenVINO release notes](https://docs.openvino.ai/2026/about-openvino/release-notes-openvino.html)
- [intel/linux-npu-driver releases](https://github.com/intel/linux-npu-driver/releases)
- [Evaluating Generated Commit Messages with LLMs](https://arxiv.org/pdf/2507.10906)
- [AI-Driven Code Documentation: LLMs for Commit Message Generation (CommitBench)](https://doi.org/10.3390/computers15020087)
- [CommitLLM](https://arxiv.org/html/2607.17532v1)
- [CodeIF: instruction following for code](https://arxiv.org/pdf/2502.19166)

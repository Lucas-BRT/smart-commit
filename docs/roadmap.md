# Roadmap

## Done

- Build provenance: the release workflow attests the assets (GitHub artifact
  attestations), and `install.sh --verify` checks them with `gh`.
- Repository style detection, diff compaction, prompt and sanitization.
- CLI with retry and fallback, traces and `replay`.
- A retry when the reply repeats a history subject, and lockfile-only
  changes written from the file list without loading the model.
- `prepare-commit-msg` hook for any git client.
- OpenVINO GenAI backend on the NPU, the Intel GPU and the CPU, with NPU
  tuning (chunked prefill, `BEST_PERF`).
- llama.cpp backend with Vulkan for AMD, NVIDIA and Intel GPUs, and on the
  CPU, so machines without Intel devices need no OpenVINO.
- `auto` device selection, `bench` and `device`; `bench` measures on
  prompts sized like real ones.
- Recommended models downloaded from Hugging Face, pinned and checked
  (`smart-commit model`), or any GGUF file from disk.
- The first fine-tunes, `smart-commit-1.5b-v0.1` and
  `smart-commit-0.5b-v0.1`, and the v0.2 pair, `smart-commit-0.5b-v0.2` and
  `smart-commit-1.5b-v0.2`, trained without the rules in their prompt, and
  `smart-commit-0.5b-v0.3` and `smart-commit-1.5b-v0.3`, the v0.1 models trained
  further on their own samples, as experimental models.
- Fast load for llama.cpp (`--no-repack --fit off`), turned on per device
  by the benchmark, and `--threads-batch` passed explicitly.
- Global and per-repository configuration; the setup removes global
  values a version cannot read (a model added by a newer build) instead of
  stopping.
- Installer, guided setup that lists the hardware first and installs only
  what each vendor's CPU, GPU and NPU need, reproducible model export and
  release packaging, automatic releases.
- `smart-commit eval` and `prompt --inputs`, which run the pipeline on
  recorded inputs; the eval-v1 and eval-short sets built with them
  (`training/`), and the paths of each history commit in traces.
- Commit scenarios (`training/scenarios/`): hand-written cases of git
  operations by level of complexity on synthetic repositories, split
  between training and evaluation.
- Messages that keep to git's own: the message of a merge, revert or
  cherry-pick in progress, a subject cut where it still reads
  (`style.subject_max`), a retry for a reply that nearly copies an earlier
  subject or stops mid-sentence, and never a message the user turned down.
- A file list with what the line counts do not say (status, renames,
  whitespace-only and comment-only changes), on by default.
- `smart-commit update` and `smart-commit uninstall`.
- The binary in modules (`cli/`, and the library's `backend/`, `diff/`,
  `models/`...), with `anyhow` and `tracing`, and `-v` for debug output.
- The setup, the hardware inventory and the llama.cpp install in the binary
  (no shell twin of the plan), and the developer tools in crates of their own
  that the release does not build: `sc-tools` (llama-server timing, release
  notes) and `sc-training` (training inputs, selection, scoring, scenarios).
- The v0.2 fine-tunes, `smart-commit-0.5b-v0.2` and `smart-commit-1.5b-v0.2`,
  trained without the rules in the prompt and evaluated. They are on
  Hugging Face, private.

## Next

Highest priority first: what users hit now and what unblocks other items,
then the largest gains for their cost.

| # | Item | Why |
|---|---|---|
| 1 | **Train v0.4** | v0.2 (training H5, H6, H7's release bumps, no type cap) and v0.3 (v0.1 plus DPO on its own samples, row 18) are published and registered. v0.4 is next, planned in [H13](../training/HYPOTHESES.md): the annotated file list, the scope map (H4b), cleaner and more even labels and 40k examples for the 1.5B, then DPO on the best of them. Without the rules, prompts are 17–21% shorter on eval-v1 and eval-short (`--rules omitted`, already in `prompt` and `eval`). See [training](training.md). |
| 2 | **A model server that stays up** (`model.keep_loaded`, on by default for 10 minutes on `vulkan` and `cpu`) | Load is paid at every commit, most of it on a GPU (weights to the GPU, shaders compiled; see [measurements](measurements.md#keeping-the-model-loaded)). Keeping `llama-server` running after a message, and stopping it after some idle minutes (say 10), makes every later commit pay only prefill and decoding. It also gives prefix caching for free: llama-server keeps the KV cache of the prompt's unchanged beginning in memory. Costs: the model's RAM or VRAM while it is up, and its lifecycle (restart when the settings or the model change, two commits at once, a stale socket). The commit nudges would need a loaded model at hand too. Done: it is on by default, since the replies were the same in the hardware tests, and only where the model takes at most 15% of the RAM and leaves a tenth available (`model.keep_loaded_max_ram`), with the server stopping itself when the machine runs short; left: the NPU and the Intel GPU, which have no server, and the prompt cache. |
| 3 | **A fine-tune as the default** | v0.2's evaluation is done, and `smart-commit-1.5b-v0.1` is still the best at the type on eval-v1 (against the default `qwen2.5-coder-1.5b`, in [training](../training/HYPOTHESES.md)); the 0.5B v0.2 is the fastest. Making one `validated` and the default (on `vulkan` and `cpu`, which need no OpenVINO export) needs the fixture prompts and the hardware tests. |
| 4 | **Fine-tunes and more models on the OpenVINO devices** | The NPU and the Intel GPU run only `qwen2.5-coder-1.5b`, the one model with a pinned export. Export the fine-tunes' published safetensors, and other [recommended models](models.md), with the pinned pipeline: two identical exports, `MANIFEST.sha256` and the **Model** workflow for each. |
| 5 | **Evaluation coverage** | eval-v1 and eval-short are English, Conventional or free; the scenarios add git states, by hand (some of them in Portuguese, which the product no longer supports). Add gitmoji cases from permissively licensed repositories, pinned by commit, and an LLM judge for faithfulness with a pinned prompt, validated against a sample rated by hand (`score.py` has only cheap checks). |
| 6 | **GBNF grammar for the conventions** | A grammar in the request makes `type(scope): description` (or a gitmoji) the only possible shape, so the format retry, which doubles the time when it happens, goes away for every model. Less pressing with the fine-tunes, whose first reply passes 95–98% of the time on eval-v1 (91–93% for the base 1.5B). |
| 7 | **Golden replies per device** | `cli/tests/fixtures/golden/<model>/<device>.json`, written only by a script; the hardware tests would flag when a driver or runtime update changes replies. |
| 8 | **Commit nudges** (`smart-commit status`, `watch`, `lsp`) | Work that starts as one small feature and drifts into tangents ends as one bloated commit: hard to review, and hard for the model to describe. One engine compares the working tree and the index with `HEAD` and says when it is time to commit; several fronts use it. Deterministic signals first, cheap enough to run all the time: diff size, how many areas are touched (top-level directories, modules, code / docs / config / lockfiles), and **drift from the focus** (the first files edited, or one declared with `smart-commit focus "..."` or taken from the branch). Then, debounced and only when the diff changed enough, the model's "one purpose or several?" judgement, once a trained model has one (training H-split). Fronts in order: `status`, one short line for shell prompts (starship, tmux); `watch`, desktop notifications; `lsp`, a stdio language server with a message and code actions ("commit the focus files now", "move this tangent to a branch or stash"). Nudge, do not nag: rate limits, snooze, off per repository, and always a concrete action rather than a bare warning. Thresholds in `config::KEYS`; the clock injectable so tests stay deterministic; new dependencies (file watching, LSP) in commits of their own. |
| 9 | **Vulkan pipeline cache** | Part of the Vulkan load is compiling shaders. Measure whether llama.cpp's Vulkan backend can reuse a pipeline cache between runs, and how much it saves. |
| 10 | **Ask less, know more about the type** | `model.ask_type` is done for `vulkan` and `cpu` in a terminal. Next: the OpenVINO devices (the C API of OpenVINO GenAI shows no log probabilities, so they would need a layer of our own); a hint from the user (`--why "..."`) or from the branch and issue, the signals the diff lacks (the branch is not in eval-v1: it needs a set from pull requests); and a key in the review screen to change the type and write the message again. |
| 11 | **Fewer history examples for trained models** | Up to 5 recent subjects (about 100 tokens) show the style; a model trained on the task may need fewer. Measure with `eval` (eval-v1 and the young-repository slice) before changing the default. |
| 12 | **Energy** | Joules per message on each device, from the package RAPL counter (it covers CPU, GPU and NPU). It needs root, either through a udev rule for the `render` group or by running the benchmark with `sudo`. With energy, `auto` could weigh more than time. |
| 13 | **CUDA and ROCm builds of llama.cpp** | Vulkan is enough everywhere; CUDA or ROCm may be faster on discrete GPUs. Measure first. |
| 14 | **Several GPUs** | `bench` measures one Vulkan device (`model.vulkan_device`); it could measure each. |
| 15 | **Other distributions** | The setup installs system packages with `apt` only and lists them elsewhere. |
| 16 | **OpenVINO 2026.4+** | When an NPU driver is validated with it. Newer runtimes also read the models the OpenVINO organisation publishes. |
| 17 | **The fixed prompt prefix cached on disk** (only if a server that stays up is not wanted) | Without a server that stays up, llama-server can save a slot's KV cache to a file and restore it (`--slot-save-path`), about 28 KB per token for the 1.5B model, 7 MB for 250 tokens. It needs the prompt to start with the part that never changes (the rules), then the repository's part. Same reproducibility check as 12. Little left to cache if the trained model drops the rules (training H6). |
| 18 | **DPO on the models' own samples** | Preference training on pairs the model sampled itself removes the replies that copy the history, invent a detail or fall back (on eval-v1, `ungrounded` 0.104 → 0.008, `first_try` 0.960 → 0.996) at no cost to the type or the scope, but does not improve the type significantly; see [training](../training/HYPOTHESES.md), H12. `smart-commit-0.5b-v0.3` and `smart-commit-1.5b-v0.3` are it; at 1.5B the type also rises a little (+11/−3, p = 0.057). Whether a version trained on the annotated file list (v0.4) takes it as a last step is open. |

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
| SmolLM3-3B | Apache-2.0 | new in 2026.3 | weak on code in published numbers |
| Phi-4-mini-instruct (3.8B) | MIT | not listed (Phi-3.5 has an official example) | strong on code |
| Gemma 4 E2B-it | Apache-2.0 | supported since 2026.2 | multimodal, 5.1B parameters in total |
| Qwen3.5-0.8B / 2B | Apache-2.0 | no (CPU and GPU only) | multimodal, needs `VLMPipeline` and a newer runtime |

Excluded: Qwen2.5-Coder-3B (research licence, not open source) and
Llama 3.2 3B (gated, custom licence).

The OpenVINO organisation's ready-made conversions are not an option on
the current runtime: they target newer tooling (the LFM2.5 one crashed, and
Qwen3.5 is multimodal), and the small ones are INT4 asymmetric, which the
NPU does not take. Models are exported with the pinned toolchain instead.

## Sources

- [OpenVINO GenAI on NPU](https://docs.openvino.ai/2026/openvino-workflow-generative/inference-with-genai/inference-with-genai-on-npu.html)
- [OpenVINO release notes](https://docs.openvino.ai/2026/about-openvino/release-notes-openvino.html)
- [intel/linux-npu-driver releases](https://github.com/intel/linux-npu-driver/releases)
- [Evaluating Generated Commit Messages with LLMs](https://arxiv.org/pdf/2507.10906)
- [AI-Driven Code Documentation: LLMs for Commit Message Generation (CommitBench)](https://doi.org/10.3390/computers15020087)
- [CommitLLM](https://arxiv.org/html/2607.17532v1)
- [CodeIF: instruction following for code](https://arxiv.org/pdf/2502.19166)

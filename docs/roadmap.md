# Roadmap

## Done

- Repository style detection, diff compaction, prompt and sanitization.
- CLI with retry and fallback, traces and `replay`.
- `prepare-commit-msg` hook for any git client.
- OpenVINO GenAI backend on the NPU, the Intel GPU and the CPU, with NPU
  tuning (chunked prefill, `BEST_PERF`).
- llama.cpp backend with Vulkan for AMD, NVIDIA and Intel GPUs, and on the
  CPU, so machines without Intel devices need no OpenVINO.
- `auto` device selection, `bench` and `device`.
- Recommended models downloaded from their publishers on Hugging Face,
  pinned and checked (`smart-commit model`), or any GGUF file from disk.
- Fast load for llama.cpp (`--no-repack --fit off`), turned on per device
  by the benchmark.
- Global and per-repository configuration.
- Installer, guided setup that lists the hardware first and installs only
  what each vendor's CPU, GPU and NPU need, reproducible model export and
  release packaging, automatic releases.

## Next

| # | Item | Why |
|---|---|---|
| 1 | **Quality evaluation** (`smart-commit eval`) | Choosing a model or changing the prompt needs numbers. A versioned set of (diff, human message, history) cases from permissively licensed repositories, pinned by commit, in several conventions and both languages. Deterministic metrics first: convention followed, subject ≤ 72, language right, ticket when expected, retry and fallback rates, and **copied examples** (the 1.5B model sometimes repeats a history subject when the diff says little). Then, optionally, a larger local model as a judge with a pinned prompt. |
| 2 | **Golden replies per device** | `tests/fixtures/golden/<model>/<device>.json`, written only by a script; the hardware tests would flag when a driver or runtime update changes replies. |
| 3 | **Energy** | Joules per message on each device, from the package RAPL counter (it covers CPU, GPU and NPU). It needs root, either through a udev rule for the `render` group or by running the benchmark with `sudo`. With energy, `auto` could weigh more than time. |
| 4 | **More models** (after 1) | OpenVINO exports for more of the [recommended models](models.md), and validating the experimental ones with the evaluation. |
| 5 | **Prompt work** (after 1) | For example an instruction against copying examples. Each change must improve the evaluation without hurting the format metrics. |
| 6 | **CUDA and ROCm builds of llama.cpp** | Vulkan is enough everywhere; CUDA or ROCm may be faster on discrete GPUs. Measure first. |
| 7 | **Several GPUs** | `bench` measures one Vulkan device (`model.vulkan_device`); it could measure each. |
| 8 | **Other distributions** | The setup installs system packages with `apt` only and lists them elsewhere. |
| 9 | **OpenVINO 2026.4+** | When an NPU driver is validated with it. Newer runtimes also read the models the OpenVINO organisation publishes. |

## Model candidates

There is no public benchmark of small models for commit messages, and recent
work (CommitBench, CommitLLM) suggests that fitting the repository's context
matters more than model size. The choice therefore waits for the evaluation
(item 1). Candidates of at most 5B parameters, which fit 15 GiB of RAM in
INT4:

| Model | Licence | On the NPU (OpenVINO 2026.3) | Notes |
|---|---|---|---|
| Qwen2.5-Coder-1.5B-Instruct (current) | Apache-2.0 | yes, validated | fast and code-focused; copies history examples when the diff says little |
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

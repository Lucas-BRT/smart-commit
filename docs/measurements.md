# Measurements

Every timing in the documentation comes from this page; the other pages link
here instead of repeating numbers. How well the fine-tuned models write
(type match, copies, fallbacks) is measured differently, with `score.py` on
eval-v1, and lives in [training/HYPOTHESES.md](../training/HYPOTHESES.md).
Console output shown in other pages is a run recorded here.

`smart-commit bench` measures every device of a machine on four prompts
embedded in the binary, sized like real ones, so the numbers of different
machines compare; the sections below say what else was run, and how to repeat
it.

## Per message, by device

Core Ultra 5 225H (Arrow Lake-H: Arc iGPU, NPU 37xx), 15 GiB RAM,
Pop!_OS 24.04, kernel 7.2. The prompts are the trace fixtures. These
measurements predate the llama.cpp `cpu` device; `openvino-cpu` is what
was then called `cpu`.

End to end through the CLI (release build, warm caches, one staged file,
mean of 3 runs; the same message on every device):

| Device | Time | CPU time used | Memory |
|---|---|---|---|
| intel-gpu | **1.8 s** | 2.1 s | 0.49 GB |
| vulkan (iGPU through Mesa ANV) | 2.5 s | 1.5 s | — |
| openvino-cpu | 4.0 s | 11.1 s (2.75 cores) | 2.0 GB |
| npu | 5.1 s | 4.1 s | 2.0 GB |

Per prompt:

| Device | Time to first token | Per token |
|---|---|---|
| intel-gpu | 60–360 ms | 35 ms |
| vulkan | 270–390 ms | 58 ms |
| openvino-cpu | 100–540 ms | 30 ms |
| npu (tuned) | ~1.07 s | 74 ms |

Ryzen 5 5500 (6 cores), GeForce RTX 3050 8 GB with the proprietary driver
595.104, 23 GiB RAM, Pop!_OS 24.04, kernel 7.2; `smart-commit bench` on
the four prompts sized like real ones (fast load on for both devices):

| Device | Per message | Load | Generate | CPU | Per message without fast load |
|---|---|---|---|---|---|
| vulkan (RTX 3050) | **2.4 s** | 2.1 s | 0.3 s | 0.4 cores | 2.7 s |
| cpu (llama.cpp) | 4.0 s | 0.7 s | 3.3 s | 5.7 cores | 5.1 s |

On the ~250-token prompts the benchmark used before, the CPU took 1.9 s and
was chosen; on prompts the size of real ones Vulkan is the faster.

Most of the Vulkan load left is copying the weights to the GPU (935 MiB in
1.5 s), which no load mode avoids.

- On this machine the integrated GPU is the fastest and the lightest
  device. The NPU is the slowest for a 1.5B model, but uses far less CPU than
  the CPU backend. Energy was not measured: reading RAPL needs root.
- `estimate_tokens` (characters / 3) stays above the tokenizer's count, as
  the prompt budget requires: real prompts are 3.4 to 3.75 characters per
  token. A hardware test checks it.

## Per message, by model

| Model | Size | Status | Devices | Per message, Ryzen 5 5500 (`cpu` / `vulkan` on an RTX 3050) | Per message, Core Ultra 5 225H (`cpu` / `vulkan` on its Arc iGPU) |
|---|---|---|---|---|---|
| `qwen2.5-coder-1.5b` (default) | 1.1 GB | validated | all | 4.0 s / 2.4 s | — |
| `qwen2.5-coder-0.5b` | 491 MB | experimental | `vulkan`, `cpu` | 3.1 s / 1.5 s | — |
| `qwen2.5-coder-7b` | 4.7 GB | experimental | `vulkan`, `cpu` | — | — |
| `smart-commit-1.5b-v0.1` | 986 MB | experimental | `vulkan`, `cpu` | 3.8 s / 2.3 s | — |
| `smart-commit-0.5b-v0.1` | 397 MB | experimental | `vulkan`, `cpu` | 1.9 s / 1.2 s | — |

Times are `smart-commit bench` with fast load, on its four prompts sized like
real ones; a dash is a pair not measured on them yet. The fine-tunes are
faster than their base models mostly because their replies are shorter.
Earlier measurements, on prompts of about 250 tokens, gave the Core Ultra 5
225H 1.3–1.7 s on `cpu` and 0.8–1.6 s on `vulkan`, and the 7B model 6.7 s /
8.3 s on the Ryzen. The 7B model's time is mostly loading: on the RTX 3050,
7.8 s of 8.3 s is copying 4.4 GB to the GPU, and generating takes 0.45 s.

## Fast load

Each commit starts `llama-server`, and most of a commit is that start.
On the Ryzen 5 5500 below, two of its steps took a large share of it and
are rarely needed for a 1 GB model with a fixed context:

- **Repacking** (CPU): the weights are copied (600 MiB) and reordered for
  the CPU's vector instructions, about 0.66 s. `--no-repack` uses them
  straight from the memory-mapped file. On this AVX2 CPU generation was
  not slower without it; CPUs with AVX-512 or AMX may gain more from it.
- **Fitting** (CPU and Vulkan): every GPU is asked for its free memory, and
  the layers and context adjusted to fit, 0.3–0.4 s, even with
  `--device none`. `--fit off` skips it; a GPU short of memory then fails
  to load instead of loading fewer layers, so smart-commit loads it again
  normally.

Since the gain depends on the processor, `smart-commit bench` measures
each llama.cpp device in both modes, and fast load is used where it saved
at least 5% per message (more than the noise between runs).
`model.fast_load = on` or `off` overrides that. The backend's description,
which traces record, ends in `load=fast` when it is on. On the Ryzen 5 5500
the replies were the same in both modes, on the short prompts the benchmark
used before.

## Keeping the model loaded

Measured end to end with
`smart-commit` on one staged file (the release binary, fast load on), on
the Ryzen 5 5500 with the RTX 3050; the replies were the same either way:

| Model and device | Load every commit | Kept loaded, from the second commit |
|---|---|---|
| `qwen2.5-coder-1.5b`, `vulkan` | 2.8 s | 0.20 s |
| `qwen2.5-coder-1.5b`, `cpu` | 2.6 s | 2.0 s |
| `smart-commit-0.5b-v0.1`, `vulkan` | 1.4 s | 0.14 s |
| `smart-commit-0.5b-v0.1`, `cpu` | 1.3 s | 0.8 s |
| `qwen2.5-coder-7b`, `vulkan` | 7.5 s | 0.57 s |

## Threads on the CPU

Reading the prompt (prefill) is most of the time per message on the CPU,
and it scales with the threads doing it. llama.cpp picks that count itself,
and the pick depends on the processor, so smart-commit passes
`--threads-batch` explicitly: one thread per physical core
(`model.threads_batch` overrides it). The backend's description, which
traces record, says `threads-batch=N` on the CPU.

Measured with Qwen2.5-Coder-1.5B Q4_K_M and real prompts (llama.cpp b11381):

| Machine | llama.cpp's default | One per physical core | Replies |
|---|---|---|---|
| Core Ultra 5 225H (14 cores, no SMT), 6 prompts | 2 threads, prefill 6992 ms | 14 threads, prefill 3369 ms | identical |
| Ryzen 5 5500 (6 cores, 12 threads), 50 prompts | 2.92 s per message | 6 threads, 2.93 s per message | identical |

On the Ryzen, llama.cpp's default was already one per core; counting the
SMT threads too (12) made it slower, 3.24 s per message, with identical
replies. Changing the decoding threads (`--threads`) changed replies on the
Core Ultra, so that one keeps llama.cpp's default.

## The NPU

- **Cache.** The first load compiles the graph (11.7 s); later loads read it
  from the cache (1.7 s, 246 MB).
- **Chunked prefill.** With the library's default chunk (1024, the same as
  our prompt length) every prompt cost 1024 tokens. With chunks of 512
  (`model.npu_prefill_chunk`) the time to first token fell from 2.37 s to
  1.05 s. Chunks of 256 and 128 fail to compile with driver 1.38
  (`UnrollPermuteDMA`).
- **`GENERATE_HINT=BEST_PERF`** (`model.npu_generate_hint`) compiles slower,
  once, for about 10% faster tokens (82 to 74 ms).

## Where the time goes on the CPU and Vulkan

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

- `llama-server` was started without `--threads`/`--threads-batch`, so it
  used its own default, which on this hybrid CPU was 2 threads. Setting
  `--threads-batch` to the core count halves prefill and gave **identical
  replies** on all 6 prompts; smart-commit now passes it
  ([above](#threads-on-the-cpu)). Changing `--threads` (decode)
  changed the replies and made decode slower, so it keeps the default.
- `smart-commit bench` used the trace fixtures, whose prompts are ~250
  tokens, so it underestimated prefill: it reported ~2.6 s per message for
  `cpu` on this machine, against ~7.9 s with real prompts. It now uses
  prompts sized like real ones ([devices](devices.md#choosing-a-device)).
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

## Token estimate

The prompt budget estimates tokens as characters / 3 (`estimate_tokens`). It
stays above the tokenizer's count for most prompts, as the budget requires:
real prompts are 3.4 to 3.75 characters per token, and a hardware test
checks it. It is not above for all: on eval-v1 the estimate was up to 1.45
times short for 2–3% of the prompts (hashes, base64 and long numbers).

## Model export

On a Ryzen 5 5500 the export from scratch (uv, Python, the packages and the
source model downloaded) took 1 min 47 s and matched the manifest
([reproducibility](reproducibility.md#the-model)).

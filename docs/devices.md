# Devices

smart-commit runs the model on the NPU, a GPU or the CPU, through two
backends:

| Device | Backend | Hardware | Model |
|---|---|---|---|
| `npu` | OpenVINO GenAI | Intel NPU (Core Ultra) | Qwen2.5-Coder-1.5B-Instruct, INT4 symmetric, exported by us |
| `intel-gpu` | OpenVINO GenAI | Intel GPU (integrated or Arc) | same |
| `vulkan` | llama.cpp (`llama-server`) | any GPU with a Vulkan driver: AMD (RADV), NVIDIA (proprietary driver or NVK), Intel (ANV) | Qwen2.5-Coder-1.5B-Instruct Q4_K_M, the official GGUF |
| `cpu` | llama.cpp (`llama-server`) | any x86_64 CPU, Intel or AMD | same GGUF |
| `openvino-cpu` | OpenVINO GenAI | any x86_64 CPU | the OpenVINO model |

What each vendor needs:

| Vendor | CPU | GPU | NPU |
|---|---|---|---|
| Intel | `cpu` (llama.cpp); `openvino-cpu` too when OpenVINO is installed | `intel-gpu` (OpenVINO + Intel's compute runtime) and `vulkan` (Mesa ANV) | `npu` (OpenVINO + NPU driver + Level Zero) |
| AMD | `cpu` (llama.cpp) | `vulkan` (Mesa RADV) | not supported |
| NVIDIA | — | `vulkan` (the proprietary driver's Vulkan ICD, or Mesa NVK) | — |

OpenVINO is only needed for Intel's NPU and GPUs: its GPU plugin only drives
Intel GPUs, which is why AMD and NVIDIA go through llama.cpp. The same
llama.cpp build (b11381) runs on the CPU of any x86_64 machine, choosing the
code for the processor (Intel or AMD) at run time, so a machine without an
Intel NPU or GPU never needs OpenVINO. `openvino-cpu` exists for machines
that have OpenVINO anyway, so `bench` can compare both CPU backends.

`gpu`, the name of `intel-gpu` before 0.4, is still accepted, with a warning.

## Choosing a device

On a Ryzen 5 5500 with an RTX 3050 (no Intel device, so no OpenVINO):

```console
$ smart-commit bench
Benchmark on AMD Ryzen 5 5500: 5 prompts, after one warm-up load.

  device       per message     load  generate     CPU
  intel-gpu    unavailable: backend unavailable: OpenVINO GenAI not found (...); Intel GPUs need it; `smart-commit setup` installs it on machines with one (other GPUs use the vulkan device)
  npu          unavailable: backend unavailable: OpenVINO GenAI not found (...); the Intel NPU needs it; `smart-commit setup` installs it on machines with one
  vulkan             2.3 s    2.2 s     0.1 s    0.4x
  cpu                1.9 s    0.6 s     1.2 s    5.5x  <- selected
  openvino-cpu unavailable: backend unavailable: OpenVINO GenAI not found (...); only openvino-cpu needs it (the cpu device runs on llama.cpp); setup-npu.sh --openvino-only installs it

fast load turned on for vulkan (--no-repack --fit off): 2.6 s -> 2.3 s per message
fast load turned on for cpu (--no-repack --fit off): 2.8 s -> 1.9 s per message

smart-commit will use cpu on this machine (~1.9 s per message): the fastest, model.device = auto.
To use another device: smart-commit device use <npu|intel-gpu|vulkan|cpu|openvino-cpu>, or `auto` for the fastest.

$ smart-commit device use vulkan
model.device = vulkan  (global /home/me/.config/smart-commit/config.toml)
~2.3 s per message on this machine, in the last benchmark.

$ smart-commit device          # the last benchmark and the current choice
```

- **`model.device = auto`** (the default) tries the devices in the order of
  the last benchmark, fastest first, then in the default order
  **intel-gpu, npu, vulkan, cpu, openvino-cpu**. The first that loads wins, with a note naming
  the ones skipped. When none works, the error lists the reason for each.
- **`smart-commit bench`** measures the five devices on the five prompts of
  the trace fixtures, embedded in the binary, so every machine is measured on
  the same inputs. Each device is loaded once to warm the caches, then loaded
  again and timed. *Per message* is what a commit costs: one load and one
  generation. *CPU* is the mean number of busy cores. The report is saved to
  `$XDG_DATA_HOME/smart-commit/bench.json`, with the CPU model, the kernel,
  the version and each backend's description.
- **Fast load.** `vulkan` and `cpu` are measured twice, with and without
  [fast load](#fast-load), and keep the faster mode; the lines under the
  table say what was turned on.
- **`smart-commit device use <device>`** writes `model.device` to the global
  config and shows the estimate for that device, or a note if it failed in
  the last benchmark.
- The device that actually ran is part of the backend's description, which
  every trace records.

Run `smart-commit bench` again after installing drivers or changing the
model; `smart-commit setup` runs it at the end.

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
the replies to the five fixture prompts are the same in both modes.

## Setting up the hardware

`smart-commit setup` first lists the hardware (the CPU, each GPU with its
vendor, kernel driver and the devices it gives, and the NPU), then prints a
plan of what that hardware needs, and only that, then asks before each step
that needs `sudo`. On a Ryzen 5 5500 with an RTX 3050:

```text
==> Hardware
  CPU  AMD Ryzen 5 5500 (x86_64)
       -> cpu (llama.cpp); openvino-cpu where OpenVINO is installed
  GPU  GA106 [Geforce RTX 3050] [0000:01:00.0], driver nvidia 595.104.02
       -> vulkan (the NVIDIA driver's Vulkan ICD)
  NPU  none

==> Plan
  llama.cpp and the GGUF model (no sudo): the cpu device, and vulkan on the GPUs
  NVIDIA GPU (0000:01:00.0, proprietary driver 595.104.02): its own Vulkan ICD
  not needed: OpenVINO, the Intel NPU driver and Intel's GPU runtime, since
  there is no Intel NPU or GPU here
```

| Hardware | What the setup installs |
|---|---|
| any x86_64 machine | llama.cpp b11381 and the GGUF model, without `sudo`: the `cpu` and `vulkan` devices |
| Intel GPU | Mesa ANV; Intel's compute runtime and OpenVINO GenAI, for `intel-gpu`; the model exported for OpenVINO |
| AMD GPU | Mesa RADV |
| NVIDIA GPU | the proprietary driver's Vulkan ICD if it is missing, or Mesa NVK with nouveau |
| Intel NPU | the NPU driver, Level Zero and OpenVINO GenAI; the model exported for OpenVINO |
| AMD NPU (Ryzen AI) | nothing: listed as not supported |

OpenVINO, the NPU driver and Intel's GPU runtime are never installed on a
machine without an Intel NPU or GPU. The steps are scripts that can also run
on their own, from `~/.local/share/smart-commit/scripts/` (or `scripts/` in
a checkout):

| Script | Does | sudo |
|---|---|---|
| `hardware.sh` | prints the hardware list above, without changing anything | no |
| `setup-llamacpp.sh [--dry-run]` | llama.cpp and the GGUF model, both pinned by SHA-256 | no |
| `setup-gpu.sh [--dry-run] [--yes]` | the system drivers each GPU needs (below) | only for system packages |
| `setup-npu.sh` | NPU driver 1.38.0, Level Zero 1.32.0 and OpenVINO GenAI 2026.3.1, the combination Intel validated; adds you to the `render` group | yes |
| `setup-npu.sh --openvino-only` | only OpenVINO, for an Intel GPU without an NPU, or to compare `openvino-cpu` with `cpu` | yes |
| `export-model.sh [--check] [--keep]` | the model for OpenVINO, exported here from Hugging Face and checked against the manifest; about 10 GB of disk while it runs | no |
| `check-npu.sh` | checks the NPU stack without changing anything | no |

OpenVINO is installed in `/opt/intel/openvino_2026.3.1`, with the link
`/opt/intel/openvino` (where the bindings look by default) and its libraries
registered in `/etc/ld.so.conf.d`. The archive's libraries have no RUNPATH,
so without that registration they would not load in processes started
outside a shell, such as git hooks run by GUI clients.

`setup-gpu.sh` reads each GPU's vendor and kernel driver from sysfs:

| GPU | Kernel driver | Installs |
|---|---|---|
| Intel | `i915` or `xe` | Intel's compute runtime for OpenVINO (intel-opencl-icd 26.27, IGC 2.38.5, pinned to the PPA snapshot the NPU driver was validated with) and Mesa ANV |
| AMD | `amdgpu` | Mesa RADV (the old `radeon` driver is not supported by RADV: warning) |
| NVIDIA | `nvidia` (closed or open kernel modules) | nothing if the driver's Vulkan ICD is there; else `libnvidia-gl-<branch>` of the running driver. **It never changes the driver** |
| NVIDIA | `nouveau` | Mesa NVK (Turing / GTX 16xx or newer) |
| NVIDIA | none | warning: install a driver (`sudo ubuntu-drivers install`) or use nouveau |

Packages already installed are skipped, `--dry-run` only prints the plan,
and without `apt` the script lists the packages to install.

Intel's compute runtime must be installed by `dpkg`, which runs `ldconfig`:
without the `libigc.so.2` links that creates, the runtime fails with a
misleading `CL_OUT_OF_HOST_MEMORY`.

## The NPU

The NPU compiles the model for fixed shapes: a prompt of
`model.max_prompt_tokens` (1024) and a reply of `model.max_new_tokens` (128).

- **Cache.** The first load compiles the graph (11.7 s); later loads read it
  from `$XDG_CACHE_HOME/smart-commit/openvino` (1.7 s, 246 MB).
- **Chunked prefill.** The NPU processes the prompt in chunks of
  `NPUW_LLM_PREFILL_CHUNK_SIZE`, but only when `MAX_PROMPT_LEN` is larger
  than a chunk. With the library's default chunk (1024, the same as our
  prompt length) every prompt cost 1024 tokens. With chunks of 512
  (`model.npu_prefill_chunk`) the time to first token fell from 2.37 s to
  1.05 s.
- **`GENERATE_HINT=BEST_PERF`** (`model.npu_generate_hint`) compiles slower,
  once, for about 10% faster tokens (82 to 74 ms).
- Both settings give identical replies (a hardware test checks it). Chunks of
  256 and 128 fail to compile with driver 1.38 (`UnrollPermuteDMA`).

## Measurements

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
595.104, 23 GiB RAM, Pop!_OS 24.04, kernel 7.2; `smart-commit bench`, the
same in two runs (fast load on for both devices):

| Device | Per message | Load | Generate | CPU | Per message without fast load |
|---|---|---|---|---|---|
| cpu (llama.cpp) | **1.9 s** | 0.6 s | 1.2 s | 5.5 cores | 2.8 s |
| vulkan (RTX 3050) | 2.3 s | 2.2 s | 0.1 s | 0.4 cores | 2.6 s |

Most of the Vulkan load left is copying the weights to the GPU (935 MiB in
1.5 s), which no load mode avoids.

- On this machine the integrated GPU is the fastest and the lightest
  device. The NPU is the slowest for a 1.5B model, but uses far less CPU than
  the CPU backend. Energy was not measured: reading RAPL needs root.
- `estimate_tokens` (characters / 3) stays above the tokenizer's count, as
  the prompt budget requires: real prompts are 3.4 to 3.75 characters per
  token. A hardware test checks it.

## Same prompt, same reply?

Decoding is always greedy, so each device gives the same reply to the same
prompt, every time (a hardware test checks it). Across devices:

- the **NPU and `openvino-cpu` give identical replies**;
- the **Intel GPU differs** on 2 of the 5 fixture prompts. It computes in f16 by
  default; forcing f32 was 12 times slower and still different, so there is
  no practical way to make it match;
- **`vulkan` and `cpu`** run a different quantisation (Q4_K_M) and differ
  too.

A trace records the device that ran, and `smart-commit replay --with-model`
notes when the current device is not the recorded one.

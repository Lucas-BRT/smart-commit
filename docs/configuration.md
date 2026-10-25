# Configuration

smart-commit works without any configuration. Settings exist to force a
style for a team, to pick a device, or to point at models in other places.

```console
$ smart-commit config init                          # ~/.config/smart-commit/config.toml, every key commented out
$ smart-commit config set model.device cpu          # global
$ smart-commit config set --repo style.convention conventional   # .smart-commit.toml, shared through git
$ smart-commit config get style.language
$ smart-commit config list --show-origin
backend = auto  (default)
model.device = cpu  (global /home/me/.config/smart-commit/config.toml)
style.convention = conventional  (repository /home/me/project/.smart-commit.toml)
...
$ smart-commit config unset model.device
$ smart-commit config path [--repo]
```

## Layers

From strongest to weakest:

1. command-line flags (`--backend`, `--device`, `--model-dir`);
2. environment variables (listed below);
3. the repository's `.smart-commit.toml`, at the root of the work tree;
4. the global `$XDG_CONFIG_HOME/smart-commit/config.toml` (by default
   `~/.config/smart-commit/config.toml`);
5. the built-in defaults.

`config list --show-origin` shows which layer each value comes from.
`config set` and `config unset` warn when a stronger layer still decides the
value, for example when an environment variable is set.

## Keys

| Key | Values | Default | Environment | Where |
|---|---|---|---|---|
| `backend` | `auto`, `openvino`, `llamacpp`, `mock` | `auto` | `SMART_COMMIT_BACKEND` | global |
| `model.device` | `auto`, `npu`, `intel-gpu`, `vulkan`, `cpu`, `openvino-cpu` | `auto` | `SMART_COMMIT_DEVICE` | global |
| `model.name` | `qwen2.5-coder-1.5b`, `qwen2.5-coder-0.5b`, `qwen2.5-coder-7b`, `smart-commit-1.5b-v0.1`, `smart-commit-0.5b-v0.1` | `qwen2.5-coder-1.5b` | `SMART_COMMIT_MODEL` | global |
| `model.dir` | path | model.name's OpenVINO export in `$XDG_DATA_HOME/smart-commit/models/` (none for models without one) | `SMART_COMMIT_MODEL_DIR` | global |
| `model.cache_dir` | path | `$XDG_CACHE_HOME/smart-commit/openvino` | `SMART_COMMIT_CACHE_DIR` | global |
| `model.gguf` | path | model.name's GGUF file in `$XDG_DATA_HOME/smart-commit/models/` | `SMART_COMMIT_GGUF` | global |
| `model.llama_server` | path | `$XDG_DATA_HOME/smart-commit/llama.cpp/llama-b11381/llama-server` | `SMART_COMMIT_LLAMA_SERVER` | global |
| `model.vulkan_device` | 0–15 | 0 | | global |
| `model.fast_load` | `auto`, `on`, `off` | `auto` | `SMART_COMMIT_FAST_LOAD` | global |
| `model.threads_batch` | 0–1024 | 0 (one per physical core) | | global |
| `model.keep_loaded` | 0–1440 | 0 | `SMART_COMMIT_KEEP_LOADED` | global |
| `model.npu_prefill_chunk` | 512–8192 | 512 | | global |
| `model.npu_generate_hint` | `best_perf`, `fast_compile` | `best_perf` | | global |
| `model.max_prompt_tokens` | 256–8192 | 1024 | | global |
| `model.max_new_tokens` | 16–1024 | 128 | | global |
| `style.convention` | `auto`, `conventional`, `gitmoji`, `free` | `auto` | | global or repository |
| `style.language` | `auto`, `english`, `portuguese` | `auto` | | global or repository |
| `style.default_convention` | `conventional`, `gitmoji`, `free` | `conventional` | | global or repository |
| `style.default_language` | `english`, `portuguese` | `english` | | global or repository |
| `style.history_depth` | 1–500 | 30 | | global or repository |
| `style.subject_max` | 40–200 | 72 | | global or repository |

- `backend = auto` uses whichever backend the device needs: OpenVINO for
  `npu`, `intel-gpu` and `openvino-cpu`, llama.cpp for `vulkan` and `cpu`.
  `mock` replies with `--mock-response` / `SMART_COMMIT_MOCK_RESPONSE`, for
  tests.
- `model.device = auto` uses the fastest device of the last
  `smart-commit bench`, and the first that works of intel-gpu, npu, vulkan,
  cpu and openvino-cpu when there is no benchmark. See [devices](devices.md).
  `gpu`, the name of `intel-gpu` before 0.4, still works with a warning;
  `config set` and `device use` write the new name.
- `model.name` is one of the [recommended models](models.md);
  `smart-commit model use <id>` downloads it and sets it. `model.gguf` and
  `model.dir` default to its files, whichever layer sets it; set them (or
  `smart-commit model use FILE`) to run a model from disk instead.
- `model.vulkan_device` picks a GPU on machines with more than one, as
  `llama-server --list-devices` numbers them (0 is `Vulkan0`).
- `model.fast_load` starts llama-server (`vulkan` and `cpu`) with
  `--no-repack --fit off`: the weights are used as they are mapped from the
  file instead of being copied and repacked for the CPU's vector
  instructions, and the model is not fitted to the GPUs' free memory, which
  a 1 GB model with a fixed context rarely needs. `auto` uses it on the
  devices where the last `smart-commit bench` found it at least 5% faster;
  without a benchmark it stays off. If a fast load fails (a GPU short of
  memory), the model is loaded again normally, with a note. See
  [devices](devices.md#fast-load).
- `model.threads_batch` sets llama-server's `--threads-batch`, the
  threads that read the prompt on the `cpu` device, which is most of the
  time per message there. 0 means one per physical core, from the CPU
  topology in sysfs. llama.cpp's own default already does that on some
  processors and not on others; see
  [measurements](measurements.md#threads-on-the-cpu). Decoding keeps
  llama.cpp's default thread count.
- `model.keep_loaded` keeps the model loaded on `vulkan` and `cpu` for that
  many minutes after a message, so the next commit skips loading it, which
  is most of a commit on a GPU. It costs the model's memory (RAM or VRAM)
  while it stays loaded. The server listens in `$XDG_RUNTIME_DIR`; without
  that variable, or if it cannot start, the model is loaded for the commit
  only, with a note. Replies are the same. `smart-commit server status`
  lists what is kept loaded, `smart-commit server stop` stops it. Changing a
  `model.*` setting with smart-commit (`config set` or `unset`, `model use`,
  `device use`) stops it too, since it runs the old settings; a load that
  fails while a model is kept says so.
- `model.npu_prefill_chunk` must divide `model.max_prompt_tokens`. Chunks
  smaller than 512 fail to compile with NPU driver 1.38.
- `model.max_prompt_tokens` is also the prompt length the NPU compiles for;
  the NPU is given a fifth less of it as the prompt budget, since it fails
  on a longer prompt and the estimate can be short
  ([architecture](architecture.md#fitting-the-diff)).
  Shorter prompts are faster on the NPU but see less of the diff; changing it
  recompiles the model once (see [measurements](measurements.md#the-npu)).
- `style.convention` and `style.language` other than `auto` override what
  the history says. With a forced convention, history examples that
  contradict it are left out of the prompt.
- `style.default_*` apply when `auto` has nothing to go on: no history, or a
  history whose language cannot be told.
- `style.subject_max` is the longest subject, in columns. 72 follows git's
  convention (GitHub cuts the subject about there, `git log --oneline` and
  patches by e-mail assume short lines); a team whose commitlint allows 100
  sets it for the repository: `smart-commit config set --repo
  style.subject_max 100`. The prompt states the limit, also to a model
  trained without the fixed rules when it is not 72, and a longer reply ends
  at the last phrase that fits (see [architecture](architecture.md)).

Two more environment variables have no key: `SMART_COMMIT_MOCK_RESPONSE`
(a reply of the mock backend) and `SMART_COMMIT_TRACE` (record every
generation to that file, see [reproducibility](reproducibility.md)).

A few more exist for tests and scripts, not for configuring a machine:
`SMART_COMMIT_SYSFS`, `SMART_COMMIT_CPUINFO` and `SMART_COMMIT_OPENVINO_DIR`
point the hardware inventory at a fake tree (`scripts/setup-gpu.sh` reads
`SMART_COMMIT_SYSFS` too), `SMART_COMMIT_HF_BASE` replaces the Hugging Face
URL models are downloaded from, and `SMART_COMMIT_LLAMACPP_URL` the one
llama.cpp is (the pinned SHA-256 is still required).

## Rules

- **Unknown keys and invalid values are errors**, with the file and the key
  in the message: a mistyped `model.devise` never goes unnoticed. In the git
  hook the error becomes a warning, so a broken file never blocks a commit.
  `smart-commit setup` removes from the global file the values this version
  cannot read, such as a model added by a newer build, with a warning that
  names the old value, so the defaults apply and the setup still runs.
- **Machine keys are global only.** `backend` and every `model.*` key are
  refused in `.smart-commit.toml`, whether by `config set --repo` or by
  editing the file. Cloning a repository must not change which model runs
  on your machine, nor make it load a model from a path someone else chose.
- **Paths** are absolute or start with `~/`, so there is no question of what
  a relative one is relative to. A relative `--model-dir` on the command line
  is resolved from the current directory, as expected from a flag.
- **`config set` keeps comments and layout.** In a file written by
  `config init`, setting a key replaces its commented line, right under its
  description.
- Settings that change what is generated (`style.*`, `model.max_prompt_tokens`,
  `model.max_new_tokens`) are recorded in traces, so `smart-commit replay`
  does not depend on the config files of whoever runs it.

Every key is declared once, in `config::KEYS`, which drives reading,
validation, `config set`, the listing and the `config init` template alike.
A test checks that this page lists every key.

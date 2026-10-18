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
| `model.dir` | path | `$XDG_DATA_HOME/smart-commit/models/qwen2.5-coder-1.5b-instruct-int4-sym-g128` | `SMART_COMMIT_MODEL_DIR` | global |
| `model.cache_dir` | path | `$XDG_CACHE_HOME/smart-commit/openvino` | `SMART_COMMIT_CACHE_DIR` | global |
| `model.gguf` | path | `$XDG_DATA_HOME/smart-commit/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf` | `SMART_COMMIT_GGUF` | global |
| `model.llama_server` | path | `$XDG_DATA_HOME/smart-commit/llama.cpp/llama-b11381/llama-server` | `SMART_COMMIT_LLAMA_SERVER` | global |
| `model.vulkan_device` | 0–15 | 0 | | global |
| `model.npu_prefill_chunk` | 512–8192 | 512 | | global |
| `model.npu_generate_hint` | `best_perf`, `fast_compile` | `best_perf` | | global |
| `model.max_prompt_tokens` | 256–8192 | 1024 | | global |
| `model.max_new_tokens` | 16–1024 | 128 | | global |
| `style.convention` | `auto`, `conventional`, `gitmoji`, `free` | `auto` | | global or repository |
| `style.language` | `auto`, `english`, `portuguese` | `auto` | | global or repository |
| `style.default_convention` | `conventional`, `gitmoji`, `free` | `conventional` | | global or repository |
| `style.default_language` | `english`, `portuguese` | `english` | | global or repository |
| `style.history_depth` | 1–500 | 30 | | global or repository |

- `backend = auto` uses whichever backend the device needs: OpenVINO for
  `npu`, `intel-gpu` and `openvino-cpu`, llama.cpp for `vulkan` and `cpu`.
  `mock` replies with `--mock-response` / `SMART_COMMIT_MOCK_RESPONSE`, for
  tests.
- `model.device = auto` uses the fastest device of the last
  `smart-commit bench`, and the first that works of intel-gpu, npu, vulkan,
  cpu and openvino-cpu when there is no benchmark. See [devices](devices.md).
  `gpu`, the name of `intel-gpu` before 0.4, still works with a warning;
  `config set` and `device use` write the new name.
- `model.vulkan_device` picks a GPU on machines with more than one, as
  `llama-server --list-devices` numbers them (0 is `Vulkan0`).
- `model.npu_prefill_chunk` must divide `model.max_prompt_tokens`. Chunks
  smaller than 512 fail to compile with NPU driver 1.38.
- `model.max_prompt_tokens` is also the prompt length the NPU compiles for.
  Shorter prompts are faster on the NPU but see less of the diff; changing it
  recompiles the model once (about 10 s).
- `style.convention` and `style.language` other than `auto` override what
  the history says. With a forced convention, history examples that
  contradict it are left out of the prompt.
- `style.default_*` apply when `auto` has nothing to go on: no history, or a
  history whose language cannot be told.

Two more environment variables have no key: `SMART_COMMIT_MOCK_RESPONSE`
(a reply of the mock backend) and `SMART_COMMIT_TRACE` (record every
generation to that file, see [reproducibility](reproducibility.md)).

## Rules

- **Unknown keys and invalid values are errors**, with the file and the key
  in the message: a mistyped `model.devise` never goes unnoticed. In the git
  hook the error becomes a warning, so a broken file never blocks a commit.
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

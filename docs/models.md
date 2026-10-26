# Models

smart-commit does not ship models. It recommends a few, downloads them from
Hugging Face when asked, and checks that each file is exactly the one it
pinned: other publishers' models from their own repositories, never
re-hosted, and smart-commit's own fine-tunes (`smart-commit-*`) from the
project's account, [LucasBRT](https://huggingface.co/LucasBRT). Any other
GGUF model can be used from disk.

In a terminal, `smart-commit model` opens a picker: the models with what is
installed and in use, the models from disk used before, the chosen one's
source and pinned commit, a progress bar while it downloads, and a path
prompt for a model on disk.
`smart-commit model list` prints the same list as text, which is also what
`smart-commit model` prints outside a terminal:

```console
$ smart-commit model list
  model                     size  licence     status        installed
  qwen2.5-coder-1.5b      1.1 GB  Apache-2.0  validated     gguf, openvino  <- in use
  qwen2.5-coder-0.5b      491 MB  Apache-2.0  experimental  -
  qwen2.5-coder-7b        4.7 GB  Apache-2.0  experimental  -
  smart-commit-1.5b-v0.1  986 MB  Apache-2.0  experimental  -
  smart-commit-0.5b-v0.1  397 MB  Apache-2.0  experimental  -
  smart-commit-0.5b-v0.2  397 MB  Apache-2.0  experimental  -
  smart-commit-1.5b-v0.2  986 MB  Apache-2.0  experimental  -
  smart-commit-0.5b-v0.3 the 0.5B v0.1 trained further on its own samples: far fewer copies, invented details and fallbacks; llama.cpp devices only (vulkan, cpu)
  smart-commit-0.5b-v0.3  397 MB  Apache-2.0  experimental  -

  qwen2.5-coder-1.5b     the default: fast on every device, code-focused
  qwen2.5-coder-0.5b     the smallest: faster, for slow CPUs; weaker messages; llama.cpp devices only (vulkan, cpu)
  qwen2.5-coder-7b       the largest: better messages, but slow to load (8 s on a GPU); llama.cpp devices only (vulkan, cpu)
  smart-commit-1.5b-v0.1 Qwen2.5-Coder-1.5B fine-tuned for smart-commit: the right commit types; llama.cpp devices only (vulkan, cpu)
  smart-commit-0.5b-v0.1 Qwen2.5-Coder-0.5B fine-tuned: as good as the 1.5B on types, faster; llama.cpp devices only (vulkan, cpu)
  smart-commit-0.5b-v0.2 the 0.5B trained without the rules in its prompt: faster, copies less in young repositories; llama.cpp devices only (vulkan, cpu)
  smart-commit-1.5b-v0.2 the 1.5B trained without the rules in its prompt: faster, copies less in young repositories, a little weaker on the type than v0.1; llama.cpp devices only (vulkan, cpu)

In use: qwen2.5-coder-1.5b (model.name, default).
Models come from Hugging Face, pinned and checked: their publishers' files,
or smart-commit's own fine-tunes (smart-commit-*).
To change: smart-commit model use <id>, or a GGUF file: smart-commit model use FILE
(remembered, and listed here while it exists)

$ smart-commit model use qwen2.5-coder-0.5b
downloading Qwen2.5-Coder-0.5B-Instruct (491 MB, Apache-2.0) from https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF
checked against the pinned SHA-256: /home/me/.local/share/smart-commit/models/qwen2.5-coder-0.5b-instruct-q4_k_m.gguf
model.name = qwen2.5-coder-0.5b  (global /home/me/.config/smart-commit/config.toml)
note: qwen2.5-coder-0.5b has no OpenVINO version: npu, intel-gpu and openvino-cpu cannot run it.
The last benchmark measured the previous model: run `smart-commit bench` again.
```

## Recommended models

| Model | Size | Status | Devices |
|---|---|---|---|
| `qwen2.5-coder-1.5b` (default) | 1.1 GB | validated | all |
| `qwen2.5-coder-0.5b` | 491 MB | experimental | `vulkan`, `cpu` |
| `qwen2.5-coder-7b` | 4.7 GB | experimental | `vulkan`, `cpu` |
| `smart-commit-1.5b-v0.1` | 986 MB | experimental | `vulkan`, `cpu` |
| `smart-commit-0.5b-v0.1` | 397 MB | experimental | `vulkan`, `cpu` |

How long each takes is in [measurements](measurements.md#per-message-by-model).

- **Validated** models are the ones the prompt was tuned on, checked on the
  fixture prompts and the hardware tests.
- **Experimental** models load and answer, but have not been through the
  checks above. On the fixture prompts the 0.5B model wrote placeholders
  (`type(diff): …`), broke the gitmoji style and copied history examples
  more often than the 1.5B model; the retry and the fallback still apply.
  The 7B model wrote the most fitting messages, but most of its time is
  loading the weights onto the GPU (see
  [measurements](measurements.md#per-message-by-model)).
- The `qwen2.5-coder-*` models are Qwen's official GGUF files (Q4_K_M),
  Apache-2.0. Models with restrictive licences are not recommended (see the
  [roadmap](roadmap.md#model-candidates)).
- The `smart-commit-*` models are Qwen2.5-Coder fine-tuned on
  smart-commit's own prompt with commits from CommitChronicle, merged and
  quantized to Q4_K_M, Apache-2.0 like their base. On eval-v1 (500 held-out
  commits) they pick the Conventional type of the real commit much more
  often than `qwen2.5-coder-1.5b`, which answers `fix` to most changes, and
  the 0.5B falls back to the file list more often on young repositories; the
  numbers are in [training/HYPOTHESES.md](../training/HYPOTHESES.md).
  Their model cards on Hugging Face list
  the evaluation, the training and the limitations. Versions are new ids
  (`-v0.2`), never replaced files.
- `smart-commit-0.5b-v0.2` was trained on prompts without the fixed rules
  (training H6): smart-commit recognises its file by the SHA-256 and sends
  it the shorter prompt, so it is about 15% faster on the CPU than v0.1
  (`bench`, whose embedded prompts carry the rules, does not show it). On
  eval-v1 it matches v0.1; in young repositories its first reply copies an
  earlier subject half as often (7.6% against 13.7%) and it falls back to
  the file list half as often (4.6% against 8.6%).
- `smart-commit-1.5b-v0.2` is the same recipe on the 1.5B: about 15% faster
  on the CPU than v0.1, and in young repositories its first reply copies
  half as often (5.6% against 11.2%). It trades quality for that: on
  eval-v1 it picks the real type a little less often than v0.1 (62.8%
  against 65.6%) and its descriptions are vaguer in young repositories, so
  v0.1 stays the better 1.5B for the type. It leans on the rules more than
  the 0.5B does.

- `smart-commit-0.5b-v0.3` is `smart-commit-0.5b-v0.1` with one more step of
  training, preference training (DPO, training H12) on its own samples, and
  keeps the full prompt. On eval-v1, with the default file list, against v0.1
  (250 Conventional commits): a reply with no word of its description in the
  diff, the paths or the history falls from 0.116 to 0.008, a first reply
  that copies an earlier subject from 0.040 to 0.004, and the file-list
  fallback from 0.028 to 0.004 (paired p < 0.05); the type (0.648 to
  0.668) and the scope (0.852 to 0.840) are within noise. It does not
  help the type: on the 47 Conventional commits of young repositories its
  `type_macro_f1` is 0.591 against 0.684. Its model card has the
  tables.

## Where they come from

Each recommended model is pinned in `src/models/registry.rs` to a Hugging Face
repository, a commit of it (never a branch) and the SHA-256 and size of the
file at that commit. `smart-commit model use` downloads it with `curl`
into `$XDG_DATA_HOME/smart-commit/models/`, resumes an interrupted
download, and refuses any file whose SHA-256 differs. A model already
there is not downloaded again.

`model.name` names the model in use; `model.gguf` and `model.dir` default
to its files. Traces record the model's file and SHA-256.

## A model from disk

```bash
smart-commit model use ~/models/my-model.gguf    # vulkan and cpu
smart-commit model use ~/models/my-openvino-dir  # npu, intel-gpu, openvino-cpu
smart-commit model use qwen2.5-coder-1.5b        # back to a recommended model
```

A file sets `model.gguf` (it must be a GGUF file), a directory with an
`openvino_model.xml` sets `model.dir`. smart-commit did not choose these
files and cannot check them: traces name them `sha256=unverified`, and the
prompt was tuned for Qwen2.5-Coder, so other models may need more retries.
A GGUF file must carry its chat template, as files converted by llama.cpp
do.

Each file or directory used this way is remembered in
`$XDG_DATA_HOME/smart-commit/local-models` (one path per line, the 10 most
recent), so switching to a recommended model and back takes one choice in
the picker, or the same `model use` again. They are listed after the
recommended models, as `unverified`, for as long as the path exists; one
that `model.gguf` or `model.dir` points at is listed too, even if set by
hand. A GGUF file in use takes the `<- in use` mark from `model.name`,
which it replaces on vulkan and cpu; an OpenVINO directory replaces it only
on its own devices, so both are marked:

```console
$ smart-commit model list
  model                            size  licence     status        installed
  qwen2.5-coder-1.5b             1.1 GB  Apache-2.0  validated     gguf, openvino
  ...
  smart-commit-0.5b-v0.2-q4_k_m  397 MB  -           unverified    gguf  <- in use
```

## The OpenVINO devices

`npu`, `intel-gpu` and `openvino-cpu` need the model exported for OpenVINO
with symmetric INT4 weights, which the NPU requires. Only models with a
pinned, reproducible export have one: today `qwen2.5-coder-1.5b`.
`smart-commit setup` exports it on machines with an Intel NPU or GPU, from
the publisher's files on Hugging Face (`scripts/export-model.sh`, about
10 GB of disk while it runs, a few minutes), and checks the result against
the committed manifest; see [reproducibility](reproducibility.md).
With another model those devices are unavailable, with a message saying so,
and `auto` uses `vulkan` or `cpu`.

## Adding a recommended model

1. An open licence, and the publisher's own GGUF file, or for a fine-tune
   of this project, its repository under the project's account (AGENTS.md,
   "Where models come from").
2. Its repository commit, file size and SHA-256 (the Hugging Face API lists
   them: `https://huggingface.co/api/models/<repo>?blobs=true`), in
   `src/models/registry.rs`.
3. `smart-commit model use <id>` and `smart-commit bench` on real hardware,
   with the numbers above updated from the benchmark.

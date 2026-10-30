# Reproducibility

Every result, whether a build, a test or a generated message, can be
reproduced by someone else on another machine. This page lists what is
pinned and how each guarantee is checked.

## Builds and releases

- The Rust toolchain is pinned in `rust-toolchain.toml`, `Cargo.lock` is
  versioned, and CI builds with `--locked`.
- **Release tarballs are byte-identical on any machine.**
  `scripts/package.sh` builds in the container of `scripts/build/Dockerfile`,
  which pins the image by digest, `gcc` and `libc6-dev` (whose C runtime
  objects are linked into the binary) by an Ubuntu snapshot date, `rustup`
  by SHA-256, and the toolchain by `rust-toolchain.toml`. Paths are remapped
  out of the binary and debug info is stripped. The archive is made in the
  same container, because `tar` and `gzip` versions change the bytes too,
  with a fixed order, owner, file modes and dates (the commit's).
  CI keeps the container's toolchain and downloaded crates between runs
  (`SMART_COMMIT_BUILD_CACHE`), never its build, so the hash it prints is
  that of a build from scratch; two packagings with and without the cache
  gave the same archive.
- *How it was checked:* v0.2.0, built on the host, differed between CI
  (GCC 11) and a developer machine (GCC 13) and in the directory modes (the
  packager's umask). With the binary built in the container, v0.3.0's
  contents matched but its archive did not: the release runner (Ubuntu
  22.04) and the developer machine (24.04) have different `tar` and `gzip`.
  With the archive made in the container too, a developer machine
  reproduces the published v0.3.0 tarball exactly. CI packages every commit
  twice and prints the hash (`cli/tests/scripts/test-install.sh`).
- `SMART_COMMIT_PACKAGE_HOST=1` builds on the host, without Docker, at the
  cost of cross-machine reproducibility.
- **CI pins what it runs on and with.** Every job names its runner image
  (`ubuntu-24.04`; the release `ubuntu-22.04`, for the oldest supported
  glibc), never `ubuntu-latest`. Actions are pinned by commit, with the
  release in a comment (`actions/checkout@<sha> # v4.4.0`): a tag can be
  moved to other code, a commit cannot. shellcheck is downloaded by version
  and SHA-256 (0.11.0), as the runner's own changes with its image and a
  newer one brings new warnings. Moving any of them to a newer release is a
  commit of its own, like a dependency update.
- What CI still takes from the runner image: `rustup` (it only installs the
  toolchain `rust-toolchain.toml` pins), Docker (it builds the pinned
  container above) and `gh` (it publishes the release). The build
  container's `ca-certificates` come from Ubuntu's live archive, since the
  snapshot service needs HTTPS; they never reach the binary.

## The model

`scripts/export-model.sh` exports Qwen2.5-Coder-1.5B-Instruct for OpenVINO,
pinning:

- the model's Hugging Face revision (downloaded with `hf download`, since
  `optimum-cli` cannot pin one);
- Python 3.11.17 (through `uv`);
- every Python package by hash in `scripts/export/requirements.lock`
  (the two git dependencies, `optimum` and `optimum-onnx`, by commit). The
  combination is the one OpenVINO GenAI 2026.3.1's archive ships in
  `samples/export-requirements.txt`, with `openvino` equal to the runtime, so
  the exported tokenizer matches the runtime's `openvino-tokenizers`;
- the NPU's flags: `--weight-format int4 --sym --group-size 128 --ratio 1.0`.

The result must match `scripts/export/MANIFEST.sha256` byte for byte, or the
script fails. The manifest's hash is the model's identity in traces. uv
0.13.1 is pinned too, by SHA-256, and downloaded into the cache when it is
missing.

*How it was checked:* two exports from scratch are identical. On CI the
weights matched but two files did not, which led to two fixes:

- `openvino_config.json` recorded the absolute path of the source directory.
  The export now runs from inside that directory with a relative path.
- OpenVINO names the nodes it creates with a process-wide counter
  (`Convert_110687` here, `Convert_110688` on CI).
  `scripts/export/normalize_ir.py` renumbers them in order of appearance;
  nothing refers to nodes by those names, and the replies did not change.

smart-commit re-hosts no publisher's model: `smart-commit setup` runs the
export on each machine that has an Intel NPU or GPU, from the publisher's
files on Hugging Face, and the manifest check makes the result the same
everywhere. It needs
about 10 GB of disk while it runs; the Python environment and the source
model are removed afterwards (`--keep` keeps them). The **Model** workflow
(`.github/workflows/model.yml`) runs the export on CI after any change to
it, to check that it reproduces on another machine. How long it takes
is in [measurements](measurements.md#model-export).

The GGUF models for llama.cpp are files on Hugging Face, each pinned in
`cli/src/models/registry.rs` to a repository commit and to the file's SHA-256 at that
commit: `smart-commit model use` refuses any other file. They are the
publishers' own files (Qwen's for the `qwen2.5-coder-*` models), or for
smart-commit's fine-tunes (`smart-commit-*`) the project's own, published
under its account. A model chosen from disk (`model.gguf`) is not checked;
traces name it `sha256=unverified`.

Training a fine-tune is not bit-for-bit reproducible (GPU kernels are not
deterministic), so its weights are a pinned input like any publisher's
file: published with the exact data, recipe and scripts (`training/`),
never retrained in place. A new training is a new version, a new
id. Generating with it is reproducible like with any other model.

## System dependencies

Every download in the setup scripts is pinned by version and SHA-256 (taken
from the publisher where it exists, computed otherwise from immutable
sources):

| Component | Version |
|---|---|
| Intel NPU driver | 1.38.0 |
| Level Zero loader (`libze1`) | 1.32.0, from the PPA snapshot the driver release names |
| OpenVINO GenAI archive | 2026.3.1, the version the NPU driver was validated with |
| Intel GPU compute runtime | intel-opencl-icd 26.27.39122.14, IGC 2.38.5, gmmlib 22.10.0, same snapshot |
| llama.cpp | b11381, Vulkan build (it runs the `cpu` device too), in `cli/src/backend/llamacpp/install.rs` |
| Qwen2.5-Coder-1.5B-Instruct | revision `2e1fd397` (OpenVINO export), `f86cb2c1` (GGUF) |

To change one, change its version and hash together.

## Training and mining on Kaggle

Part of the training (`training/`) runs on Kaggle's T4s, and what runs there is
pinned like the rest, with the limits of a machine that is not ours. The
quota and the sessions are in [training](training.md#kaggles-quota).

| Component | How it is pinned |
|---|---|
| llama.cpp for the mining | b11381, the CUDA 12.8 build and its runtime libraries (`llama-b11381-bin-ubuntu-cuda-12.8-x64.tar.gz` and `cudart-llama-b11381-bin-ubuntu-cuda-12.8-x64.tar.gz`), each by SHA-256 in `training/tools/src/kaggle.rs`, checked by the notebook before it unpacks them |
| The models it samples from | `smart-commit-0.5b-v0.1` and `smart-commit-1.5b-v0.1`, read from the registry (`cli/src/models/registry.rs`) by `sc-training kaggle-bundle` (repository commit and SHA-256), downloaded and checked by size and SHA-256 |
| The base models of the training | the Qwen2.5-Coder revisions of `training/tools/src/kaggle.rs` (those `scripts/export-model.sh` pins) |
| Python packages | the versions of `training/uv.lock` (`requirements.txt` of the bundle), except torch: Kaggle's own CUDA build is kept, and `run.json` records its version. Kaggle's `torchao` is removed, as the pinned `peft` refuses it |
| The inputs of a run | the bundle holds them and the notebook checks them: the SHA-256 of each shard's prompts (mining), of the pairs and of the SFT adapter (DPO) |
| The kernel | `training/kaggle/*-kernel-metadata.json`: the image by digest, the GPU, the internet and that it is private; `dataset_sources` is the bundle's dataset of the run |

What is recorded: `run.json` of a training has the arguments, the SHA-256 of
the pairs, the validation pairs, the SFT adapter and the base weights, the
versions of torch, transformers, peft and accelerate, the CUDA version, the GPU
and its driver, Python, the seconds and the log of the losses; the notebooks keep each job's
log.

What is not reproducible, and is treated as an input instead: the weights a
training makes (GPU kernels are not deterministic, as above), and so the DPO
adapters. The sampling of the mining is deterministic for a model, a llama.cpp
build, a backend and a GPU, because every reply has a seed made from the
input's number and the sample's; whether the CUDA candidates of the second run
equal the Vulkan ones of the first for the same input has not been checked,
and the first run's were sampled on an RTX 3050 through Vulkan. The pairs
made from candidates are deterministic, and so are the balanced sets
(`sc-training balance` takes its pairs in the order of a hash, not of the
files).

## Generation

- Decoding is greedy, and every parameter is explicit in `GenerationConfig`
  and in what each backend sends, never a library default. There is no seed,
  because nothing is sampled (llama.cpp gets `seed: 0` anyway).
- The retry after a rejected reply changes the prompt deterministically, by
  adding why the reply was rejected, instead of hoping for a different
  answer.
- Each device gives the same reply to the same prompt every time. Across
  devices, the NPU and the CPU agree, while the GPU (f16) and Vulkan (another
  quantisation) do not; see [devices](devices.md#same-prompt-same-reply).

## Traces

```console
$ smart-commit --trace t.json             # or SMART_COMMIT_TRACE=t.json git commit
$ smart-commit replay t.json              # the pipeline again, with the recorded replies
$ smart-commit replay t.json --with-model # asks the model again
```

A trace holds what git reported (branch, subjects and the paths each one
touched, staged files and diff),
the prompt options, the generation config, the backend's description (device,
model identity, NPU shapes) and every attempt: prompt, raw reply and why it
was rejected, and, when `model.ask_type` made a person choose the type, what
was offered with its probabilities and what was chosen (the replies begin
with it). It leaves out the repository's path. `replay` rebuilds
everything from it and reports any difference in prompts, rejections or
message.

`cli/tests/fixtures/traces/` holds traces made by `cli/tests/fixtures/make-traces.sh`
in repositories with a fixed identity, dates and config, and
`cli/tests/replay_fixtures.rs` requires them all to replay. The script must be
the only way they change, and their diff is reviewed. A bad message found in
real use becomes a new fixture.

## Tests

- `cargo test` needs no hardware, network, clock or randomness, and ignores
  the machine's git config, smart-commit config, models and `SMART_COMMIT_*`
  variables (see `cli/tests/common::isolate`). Test repositories have fixed
  authors and dates, so even commit hashes are stable.
- `cli/tests/scripts/` runs the shell scripts against fake sysfs trees, fake
  releases and stub binaries.
- `cli/tests/hardware.rs` (ignored by default) runs the real model on the fixture
  prompts: replies are non-empty and stable, the token estimate never
  undercounts, the NPU tuning does not change replies, and neither does a
  model kept loaded (`model.keep_loaded`).

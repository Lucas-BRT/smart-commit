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
- *How it was checked:* v0.2.0, built on the host, differed between CI
  (GCC 11) and a developer machine (GCC 13) and in the directory modes (the
  packager's umask). With the binary built in the container, v0.3.0's
  contents matched but its archive did not: the release runner (Ubuntu
  22.04) and the developer machine (24.04) have different `tar` and `gzip`.
  With the archive made in the container too, a developer machine
  reproduces the published v0.3.0 tarball exactly. CI packages every commit
  twice and prints the hash (`tests/scripts/test-install.sh`).
- `SMART_COMMIT_PACKAGE_HOST=1` builds on the host, without Docker, at the
  cost of cross-machine reproducibility.

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

smart-commit hosts no models: `smart-commit setup` runs the export on each
machine that has an Intel NPU or GPU, from the publisher's files on Hugging
Face, and the manifest check makes the result the same everywhere. It needs
about 10 GB of disk while it runs; the Python environment and the source
model are removed afterwards (`--keep` keeps them). The **Model** workflow
(`.github/workflows/model.yml`) runs the export on CI after any change to
it, to check that it reproduces on another machine. On a Ryzen 5 5500 the
export from scratch (uv, Python, the packages and the source model
downloaded) took 1 min 47 s and matched the manifest.

The GGUF models for llama.cpp are files on Hugging Face, each pinned in
`src/models.rs` to a repository commit and to the file's SHA-256 at that
commit: `smart-commit model use` refuses any other file. They are the
publishers' own files (Qwen's for the `qwen2.5-coder-*` models), or for
smart-commit's fine-tunes (`smart-commit-*`) the project's own, published
under its account. A model chosen from disk (`model.gguf`) is not checked;
traces name it `sha256=unverified`.

Training a fine-tune is not bit-for-bit reproducible (GPU kernels are not
deterministic), so its weights are a pinned input like any publisher's
file: published with the exact data, recipe and scripts (on the `training`
branch), never retrained in place. A new training is a new version, a new
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
| llama.cpp | b11381, Vulkan build (it runs the `cpu` device too), in `setup-llamacpp.sh` |
| Qwen2.5-Coder-1.5B-Instruct | revision `2e1fd397` (OpenVINO export), `f86cb2c1` (GGUF) |

To change one, change its version and hash together.

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

A trace holds what git reported (branch, subjects, staged files and diff),
the prompt options, the generation config, the backend's description (device,
model identity, NPU shapes) and every attempt: prompt, raw reply and why it
was rejected. It leaves out the repository's path. `replay` rebuilds
everything from it and reports any difference in prompts, rejections or
message.

`tests/fixtures/traces/` holds traces made by `tests/fixtures/make-traces.sh`
in repositories with a fixed identity, dates and config, and
`tests/replay_fixtures.rs` requires them all to replay. The script must be
the only way they change, and their diff is reviewed. A bad message found in
real use becomes a new fixture.

## Tests

- `cargo test` needs no hardware, network, clock or randomness, and ignores
  the machine's git config, smart-commit config, models and `SMART_COMMIT_*`
  variables (see `tests/common::isolate`). Test repositories have fixed
  authors and dates, so even commit hashes are stable.
- `tests/scripts/` runs the shell scripts against fake sysfs trees, fake
  releases and stub binaries.
- `tests/hardware.rs` (ignored by default) runs the real model on the fixture
  prompts: replies are non-empty and stable, the token estimate never
  undercounts, and the NPU tuning does not change replies.

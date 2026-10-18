# smart-commit

[![CI](https://github.com/Lucas-BRT/smart-commit/actions/workflows/ci.yml/badge.svg)](https://github.com/Lucas-BRT/smart-commit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/tag/Lucas-BRT/smart-commit?filter=v*&label=release&sort=semver)](https://github.com/Lucas-BRT/smart-commit/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.99](https://img.shields.io/badge/rust-1.99-orange?logo=rust)](rust-toolchain.toml)
[![Linux x86_64](https://img.shields.io/badge/platform-linux%20x86__64-lightgrey?logo=linux&logoColor=white)](docs/usage.md#install)

**smart-commit** writes git commit messages in the style of your repository
with a small language model that runs entirely on your machine: on an Intel
NPU, on a GPU (Intel, AMD or NVIDIA) or on the CPU. No account, no API key,
and nothing leaves your computer.

```console
$ git add -p
$ smart-commit
feat(diff): share the token budget between files by priority
```

## Features

- **Follows your repository's style.** It learns the convention
  (Conventional Commits, gitmoji or free form), the language, the scopes and
  the ticket format from the history.
- **Runs on the hardware you have.** OpenVINO drives Intel NPUs, Intel GPUs
  and CPUs; llama.cpp with Vulkan drives AMD and NVIDIA GPUs, with
  proprietary or open-source drivers. It benchmarks the machine and picks the
  fastest device.
- **Works with any git client.** A `prepare-commit-msg` hook fills in the
  message of every `git commit`, from the terminal, an editor or a GUI, and
  never blocks a commit.
- **Reproducible.** Toolchain, dependencies, drivers, runtimes and models are
  pinned by version and hash. The model export and the release tarballs are
  byte-identical on any machine, and any generation can be recorded and
  replayed.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash
```

This installs smart-commit in `~/.local` without `sudo`, then runs the
guided setup. The setup asks before installing drivers, downloads the models
and ends with a benchmark that shows which device will be used:

```text
  device   per message     load  generate     CPU
  gpu            1.0 s    0.6 s     0.4 s    1.1x  <- selected
  npu            3.4 s    1.8 s     1.6 s    0.4x
  vulkan         1.6 s    0.8 s     0.8 s    0.5x
  cpu            2.1 s    1.3 s     0.9 s    2.9x
```

Requirements: Linux on x86_64. Drivers are installed with `apt`
(Ubuntu 24.04 and derivatives are tested); on other distributions the setup
lists the packages to install. See the [usage guide](docs/usage.md) for the
installer's options and for building from source.

## Usage

| Command | Does |
|---|---|
| `smart-commit` | print a message for what is staged |
| `smart-commit --commit` | open the editor with the message, then commit |
| `smart-commit hook install` | fill in the message of every `git commit` in this repository |
| `smart-commit device` / `device use <device>` | show or change the device |
| `smart-commit bench` | measure this machine's devices |
| `smart-commit config list --show-origin` | show every setting and where it comes from |
| `smart-commit context` / `prompt` | show the detected style and the exact prompt |
| `smart-commit --trace FILE` / `replay FILE` | record and replay a generation |
| `smart-commit setup` | install drivers and models, then benchmark |

## Supported hardware

| Device | Backend | Hardware |
|---|---|---|
| `npu` | OpenVINO GenAI | Intel NPU (Core Ultra) |
| `gpu` | OpenVINO GenAI | Intel GPU, integrated or Arc |
| `vulkan` | llama.cpp | AMD (RADV), NVIDIA (proprietary or NVK) and Intel GPUs |
| `cpu` | OpenVINO GenAI | any x86_64 CPU |

The default model is Qwen2.5-Coder-1.5B-Instruct. On a Core Ultra 5 225H a
message takes 1.8 s end to end on the integrated GPU, 2.5 s through Vulkan,
4.0 s on the CPU and 5.1 s on the NPU. See [devices](docs/devices.md) for the
measurements.

## Documentation

- [Usage](docs/usage.md): install, everyday use, hook, devices, troubleshooting
- [Configuration](docs/configuration.md): every setting and its layers
- [Devices](docs/devices.md): backends, device selection, hardware setup, measurements
- [Architecture](docs/architecture.md): how a message is made
- [Reproducibility](docs/reproducibility.md): what is pinned and how it is checked
- [Roadmap](docs/roadmap.md): what comes next, and the model candidates

## Contributing

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The tests need no hardware. [AGENTS.md](AGENTS.md) describes the rules for
changes (tests with every change, one purpose per commit, reproducibility)
and how releases are made.

## License

smart-commit is released under the [MIT License](LICENSE). The models have
their own licences: Qwen2.5-Coder is Apache-2.0, and its licence is
downloaded with it.

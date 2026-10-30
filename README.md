# smart-commit

[![CI](https://github.com/Lucas-BRT/smart-commit/actions/workflows/ci.yml/badge.svg)](https://github.com/Lucas-BRT/smart-commit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/tag/Lucas-BRT/smart-commit?filter=v*&label=release&sort=semver)](https://github.com/Lucas-BRT/smart-commit/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.99](https://img.shields.io/badge/rust-1.99-orange?logo=rust)](rust-toolchain.toml)
[![Linux x86_64](https://img.shields.io/badge/platform-linux%20x86__64-lightgrey?logo=linux&logoColor=white)](docs/usage.md#install)

**smart-commit** writes git commit messages in the style of your repository
with a small language model that runs entirely on your machine: on an Intel
NPU, on a GPU (Intel, AMD or NVIDIA) or on the CPU. No account, no API key,
and nothing leaves your computer. (The one request it can make by itself, a
daily look for a newer release, is off until you turn it on:
[update notice](docs/usage.md#update-notice).)

```console
$ git add -p
$ smart-commit
feat(diff): share the token budget between files by priority
```

## Features

- **Follows your repository's style.** It learns the convention
  (Conventional Commits, gitmoji or free form), the scopes and the ticket
  format from the history. Messages are in English.
- **Runs on the hardware you have.** OpenVINO drives Intel NPUs and Intel
  GPUs; llama.cpp drives AMD and NVIDIA GPUs through Vulkan, with proprietary
  or open-source drivers, and the CPU of any x86_64 machine. The setup lists
  the hardware first and installs only what it needs (no OpenVINO without an
  Intel NPU or GPU), then benchmarks the machine and picks the fastest
  device.
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
guided setup. The setup lists the machine's CPU, GPUs and NPU, plans what
they need, asks before installing drivers, downloads the models and ends
with a benchmark that shows which device will be used:

```text
  device       per message     load  generate     CPU
  vulkan             2.4 s    2.1 s     0.3 s    0.4x  <- selected
  cpu                4.0 s    0.7 s     3.3 s    5.7x
```

(a Ryzen 5 5500 with an RTX 3050; the Intel-only devices are listed as
unavailable there.)

To read the plan before installing anything, use `--no-setup` and then
`smart-commit setup --dry-run` ([usage](docs/usage.md#install)).

Requirements: Linux on x86_64. Drivers are installed with `apt`
(Ubuntu 24.04 and derivatives are tested); on other distributions the setup
lists the packages to install. See the [usage guide](docs/usage.md) for the
installer's options and for building from source.

`smart-commit update` installs a newer release, keeping the models and the
settings; `smart-commit uninstall` removes smart-commit and everything it
downloaded, after listing it.

## Usage

| Command | Does |
|---|---|
| `smart-commit` | print a message for what is staged |
| `smart-commit --commit` | review the message (commit, edit, ask for another), then commit |
| `smart-commit hook install` | fill in the message of every `git commit` in this repository |
| `smart-commit device` / `device use <device>` | show or change the device |
| `smart-commit model` / `model use <id>` | show or change the [model](docs/models.md), including smart-commit's fine-tunes |
| `smart-commit bench` | measure this machine's devices |
| `smart-commit config list --show-origin` | show every setting and where it comes from |
| `smart-commit context` / `prompt` | show the detected style and the exact prompt |
| `smart-commit --trace FILE` / `replay FILE` | record and replay a generation |
| `smart-commit setup` | install drivers and models, then benchmark (`--dry-run` only prints the plan) |
| `smart-commit doctor` | check the installation (model, llama.cpp, device, hook, ...) and say how to fix what is wrong |
| `smart-commit hardware` | list the CPU, GPUs and NPU, and the devices each gives |
| `smart-commit server status` / `stop` | the model kept loaded between commits (`model.keep_loaded`) |

## Supported hardware

| Device | Backend | Hardware |
|---|---|---|
| `npu` | OpenVINO GenAI | Intel NPU (Core Ultra) |
| `intel-gpu` | OpenVINO GenAI | Intel GPU, integrated or Arc |
| `vulkan` | llama.cpp | AMD (RADV), NVIDIA (proprietary or NVK) and Intel GPUs |
| `cpu` | llama.cpp | any x86_64 CPU, Intel or AMD |
| `openvino-cpu` | OpenVINO GenAI | any x86_64 CPU, where OpenVINO is installed |

The default model is Qwen2.5-Coder-1.5B-Instruct. How long a message takes
on each device, model and machine is in [measurements](docs/measurements.md);
see [devices](docs/devices.md) for how the device is chosen.

## Documentation

- [Usage](docs/usage.md): install, everyday use, hook, devices, troubleshooting
- [Configuration](docs/configuration.md): every setting and its layers
- [Devices](docs/devices.md): backends, device selection, hardware setup
- [Measurements](docs/measurements.md): how long a message takes, by device, model and machine
- [Models](docs/models.md): the recommended models, where they come from, using your own
- [Architecture](docs/architecture.md): how a message is made
- [Reproducibility](docs/reproducibility.md): what is pinned and how it is checked
- [Training](docs/training.md): how smart-commit's own models are trained and measured
- [Roadmap](docs/roadmap.md): what comes next, and the model candidates

## Contributing

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The tests need no hardware. [AGENTS.md](AGENTS.md) describes the rules for
changes (tests with every change, one purpose per commit, reproducibility)
and how releases are made.

## License

smart-commit is released under the [MIT License](LICENSE). The models are
not part of it: they are downloaded from Hugging Face under their own
licences (the recommended ones are Apache-2.0), from their publishers, or
for smart-commit's own fine-tunes from the project's account.

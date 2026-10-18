# Usage

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash
```

The installer downloads the latest release, checks it against the release's
`SHA256SUMS` and installs it in `~/.local/bin` and `~/.local/share/smart-commit`,
without `sudo`. It then runs `smart-commit setup`, which:

1. lists the hardware: the CPU, each GPU with its vendor and driver, and the
   NPU, with the devices each one gives;
2. prints a plan with what that hardware needs, and nothing else: llama.cpp
   and its model everywhere (the `cpu` and `vulkan` devices, no `sudo`), the
   Vulkan driver of each GPU, and OpenVINO only for an Intel NPU or GPU;
3. installs it, asking before anything that uses `sudo`, and checks every
   download against its hash;
4. benchmarks the devices and says which one will be used.

In a terminal, `smart-commit setup` is a full-screen interface: the hardware,
then the plan, where Space turns each step on or off and `m` chooses another
[model](models.md), then each step's output, with a progress bar for
downloads, and the live benchmark at the end. Steps that use `sudo` run
outside the full screen, where `sudo` can ask for the password. Steps
already done are listed but off. With `--yes`, or without a terminal, the
same plan runs as text (`scripts/setup.sh`).

Installer options (after `bash -s --` when piping):

| Option | Effect |
|---|---|
| `--version 0.3.0` | install that release instead of the latest |
| `--prefix DIR` | install in `DIR/bin` and `DIR/share` |
| `--no-setup` | only install; run `smart-commit setup` later |
| `--yes` | answer yes to the setup's questions |
| `--uninstall` | remove smart-commit (models, caches and config are kept) |
| `--purge` | `--uninstall`, plus the models, llama.cpp, the benchmark, the caches and the config |
| `--system` | `--uninstall`, plus the OpenVINO that the setup installed (asks before using `sudo`) |

The installer warns if `~/.local/bin` is not on your `PATH`. Running it
again upgrades in place: it only replaces the binary, the scripts and the
docs, so the models, llama.cpp and the benchmark stay.
`smart-commit setup` can run again at any time, for example after adding a
GPU; `~/.local/share/smart-commit/scripts/hardware.sh` only prints the
hardware list. `--no-drivers` skips everything that needs `sudo`, and `--no-bench`
skips the benchmark.

To build from source instead:

```bash
git clone https://github.com/Lucas-BRT/smart-commit && cd smart-commit
cargo build --release --locked      # the toolchain comes from rust-toolchain.toml
target/release/smart-commit setup
```

## Everyday use

```bash
git add -p                     # choose what goes into the commit
smart-commit                   # print the suggested message
smart-commit --commit          # review it, then commit
smart-commit -C ../other-repo  # work on another repository
```

In a terminal, `--commit` shows the message, the model and device that
wrote it, how long it took and the staged files:

```text
┌ Commit message ─────────────────────────────────────────────────┐
│feat(retry): add a retry loop                                    │
│                                                                 │
│Retries failed requests three times.                             │
└─────────────────────────────────────────────────────────────────┘
 qwen2.5-coder-1.5b on cpu · 1.6 s · from the model
┌ Staged: 2 files ────────────────────────────────────────────────┐
│ M src/retry.rs                                                  │
│ R src/http.rs -> src/net.rs                                     │
└─────────────────────────────────────────────────────────────────┘
[Enter] commit  [e] edit, then commit  [a] another message  [q] cancel
```

**Another message** asks the model again with the messages turned down
in the prompt: decoding is greedy, so only a different prompt gives a
different message, and the same choices always give the same messages.
The model stays loaded, so each new message costs only the generation.
Outside a terminal (scripts, pipes), `--commit` opens the editor with the
message, as before.

The message follows the repository's style: its convention (Conventional
Commits, gitmoji or free), its language (English or Portuguese), the scopes
it uses, and the ticket in the branch name (`ABC-123`) when the history cites
tickets.

### On every `git commit`, from any client

```bash
smart-commit hook install      # in the current repository
git commit                     # the editor opens with the message written
smart-commit hook uninstall
```

The hook only writes on a plain `git commit`. With `-m`, `--amend`, a merge
or a template it does nothing. If anything goes wrong it prints a warning and
**never blocks the commit**.

### See what the model gets

```bash
smart-commit context           # the style detected in the history
smart-commit prompt            # the exact prompt for what is staged
```

`smart-commit prompt --inputs FILE` builds prompts without a repository, from
JSON Lines of the `inputs` object of [traces](reproducibility.md) (`-` reads
stdin), and prints one `{"system": ..., "user": ...}` per line. Only global
settings apply. It gives a training or evaluation set the exact prompts
smart-commit sends.

## Devices

```bash
smart-commit device            # the device in use and the measured times (choose another in a terminal)
smart-commit device use npu    # use the NPU from now on
smart-commit device use auto   # back to the fastest
smart-commit bench             # measure again
```

`auto` uses the fastest device of the last benchmark, and falls back to the
next one, with a note, if it becomes unavailable. See [devices](devices.md).

## Models

```bash
smart-commit model                       # choose a model (a picker in a terminal)
smart-commit model list                  # the recommended models, and which is in use
smart-commit model use qwen2.5-coder-7b  # download it from Hugging Face and use it
smart-commit model use ~/models/my.gguf  # use a GGUF file from disk, unverified
```

Recommended models are downloaded from Hugging Face (their publishers'
repositories, or the project's account for smart-commit's own fine-tunes),
pinned to a revision and checked against a SHA-256. See [models](models.md).

## Settings

```bash
smart-commit config list --show-origin                         # everything, and where it comes from
smart-commit config set style.language english
smart-commit config set --repo style.convention conventional   # .smart-commit.toml, shared through git
smart-commit config init                                       # a file with every key commented out
```

| To… | Run |
|---|---|
| always write in English | `smart-commit config set style.language english` |
| enforce Conventional Commits for a team | `smart-commit config set --repo style.convention conventional` |
| get faster NPU replies that see less of the diff | `smart-commit config set model.max_prompt_tokens 512` |
| use another GPU on a multi-GPU machine | `smart-commit config set model.vulkan_device 1` |
| keep replies short | `smart-commit config set model.max_new_tokens 64` |

Every key is described in [configuration](configuration.md).

## Report a bad message

```bash
smart-commit --trace bad.json  # or: SMART_COMMIT_TRACE=bad.json git commit
smart-commit replay bad.json   # checks that it reproduces
```

A trace contains the staged diff and recent commit subjects, but not the
repository's path. Review it before sharing; with it, the case can become a
test fixture.

## Troubleshooting

| Message | What to do |
|---|---|
| `no device could run the model` | `smart-commit setup`; the message lists what each device lacks |
| `no model in …`, `no GGUF model at …`, `no llama-server at …` | `smart-commit setup` downloads them; `smart-commit model use <id>` downloads a model |
| `… has no OpenVINO version, which npu needs` | the model runs on `vulkan` and `cpu` only; `smart-commit model use qwen2.5-coder-1.5b` for the NPU |
| `OpenVINO GenAI not found` | only `npu`, `intel-gpu` and `openvino-cpu` need it; on a machine without an Intel NPU or GPU, use `vulkan` or `cpu` |
| `model.device = gpu … is the old name of intel-gpu` | `smart-commit device use intel-gpu` (or `auto`) |
| `cannot load the model on the NPU` | `~/.local/share/smart-commit/scripts/check-npu.sh`; after being added to the `render` group, log out and back in |
| NVIDIA GPU `has no driver bound` | install a driver (`sudo ubuntu-drivers install`) or use nouveau, then `smart-commit setup` |
| the hook does nothing | it only acts on `git commit` without `-m`; `smart-commit hook install` reinstalls it |

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash -s -- --uninstall
```

| Option | Removes |
|---|---|
| `--uninstall` | the binary, the scripts and the docs; the models, llama.cpp, the benchmark, the caches and the config stay, for a later install |
| `--purge` | also `~/.local/share/smart-commit`, `~/.cache/smart-commit` and `~/.config/smart-commit` |
| `--system` | also OpenVINO, when the setup installed it: `/opt/intel/openvino_2026.3.1`, the `/opt/intel/openvino` link and `/etc/ld.so.conf.d/smart-commit-openvino.conf`; it lists them and asks before using `sudo` (`--yes` skips the question) |

`--purge --system` removes all of it. `--system` recognises the setup's
OpenVINO by its `ld.so.conf.d` file, so an OpenVINO installed another way is
never touched.

What always stays: the driver packages (the Intel NPU driver, Level Zero,
Intel's compute runtime, the Vulkan drivers), which other programs may use,
and membership of the `render` group. Remove them with `apt` and `gpasswd`
if nothing else needs them. The hook stays in the repositories where it was
installed: remove `.git/hooks/prepare-commit-msg` there, or run
`smart-commit hook uninstall` before uninstalling.

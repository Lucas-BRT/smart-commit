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
same plan runs as text, asking before each step that uses `sudo` (with
`--yes`, without asking; without a terminal, those steps are skipped).

Installer options (after `bash -s --` when piping):

| Option | Effect |
|---|---|
| `--version 0.3.0` | install that release instead of the latest |
| `--prefix DIR` | install in `DIR/bin` and `DIR/share` |
| `--no-setup` | only install; run `smart-commit setup` later |
| `--update` | install only if the release is not the version installed, without the setup; llama.cpp follows the new release's build (what `smart-commit update` runs) |
| `--yes` | answer yes to the setup's questions |
| `--uninstall` | remove smart-commit (models, caches and config are kept) |
| `--purge` | `--uninstall`, plus the models, llama.cpp, the benchmark, the caches and the config |
| `--system` | `--uninstall`, plus the OpenVINO that the setup installed (asks before using `sudo`) |

The installer warns if `~/.local/bin` is not on your `PATH`. Running it
again upgrades in place: it only replaces the binary, the scripts and the
docs, so the models, llama.cpp and the benchmark stay.

`smart-commit setup` can run again at any time, for example after adding a
GPU; `smart-commit hardware` only prints the hardware list. `--no-drivers`
skips everything that needs `sudo`, and `--no-bench` skips the benchmark.

To see the plan before anything is installed, install without the setup and
ask for a dry run:

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash -s -- --no-setup
smart-commit setup --dry-run
```

`smart-commit setup --dry-run` prints the hardware and the plan as text, in a
terminal too: each step, whether it needs `sudo` and for what (apt packages,
files in `/opt/intel` and `/etc/ld.so.conf.d`, the render group), what it
downloads and how big (the sizes it knows: llama.cpp and the model; the total
is printed only when every pending download has a known size), and which
steps are already done and would be skipped. It ends with `Nothing was
changed. Run `smart-commit setup` to apply it.` and changes nothing: no
download, no file or directory written, no `sudo`. It only runs the
read-only checks the setup itself uses to plan (`check-npu.sh`,
`export-model.sh --check`, `setup-gpu.sh --dry-run`). It combines with
`--no-drivers` (the plan then has no `sudo` step) and `--no-bench`; `--yes`
is accepted and ignored.

To build from source instead:

```bash
git clone https://github.com/Lucas-BRT/smart-commit && cd smart-commit
cargo build --release --locked      # the toolchain comes from rust-toolchain.toml
target/release/smart-commit setup
```

## Update

```bash
smart-commit update
```

installs the latest release if it is newer than the one running, from
GitHub's releases, checked against their `SHA256SUMS`. The models, the
settings and the benchmark stay; models kept loaded are stopped first, and
when llama.cpp is installed it follows the build the new release pins (no
`sudo`). It runs the `install.sh` that came with the release, so it works
for installs made by `install.sh` from 0.7.0 on; 0.6.0 and older update once
with `install.sh` itself, and a build from source with git and cargo.

## Everyday use

```bash
git add -p                     # choose what goes into the commit
smart-commit                   # print the suggested message
smart-commit --commit          # review it, then commit
smart-commit -C ../other-repo  # work on another repository
smart-commit -v                # also print what was decided and why
```

Errors, warnings and notes go to stderr as `error:`, `warning:` and `note:`
lines. `-v` adds `debug:` lines (rejected replies, devices that were skipped, the
fallback); `-vv` adds more. It works before or after a
subcommand: `smart-commit -v device`.

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
different message, and the same choices always give the same messages. A
reply that repeats one of them, or nearly (the same subject with a few
words dropped), is rejected and asked again, as a copy of the history is.
The model stays loaded, so each new message costs only the generation.
Outside a terminal (scripts, pipes), `--commit` opens the editor with the
message, as before.

**Asking for the type.** The type is where the message is least certain:
two readers of the same diff pick the author's type about 70% of the time.
With `smart-commit config set model.ask_type low_confidence`, `--commit`
asks which type the message should have when the model is unsure of it,
offering its likeliest ones:

```text
 Which type is this change?
 The model is not sure: it would write feat (46%).
┌──────────────────────────────────────────────────────────┐
│ 1 feat        46%                                        │
│ 2 fix         38%                                        │
│ 3 refactor    16%                                        │
│ 4 let the model decide                                   │
└──────────────────────────────────────────────────────────┘
[Enter] choose  [1-9] pick  [↑/↓] move  [q] cancel
```

The message is then written for the type chosen. It asks on about a quarter
of the commits at the default `model.ask_type_below` of 50 (percent), and
costs one more reading of the prompt on every commit it is on for. It is
off by default, and only for Conventional Commits on `vulkan` and `cpu`;
see [configuration](configuration.md) and
[measurements](measurements.md#asking-for-the-type) for what it gains.

The message follows the repository's style: its convention (Conventional
Commits, gitmoji or free), the scopes it uses, and the ticket in the branch
name (`ABC-123`) when the history cites tickets. The message is in English.

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

`smart-commit eval FILE` reads the same JSON Lines and runs the whole
pipeline on each input (prompt, model, sanitizer, retry, fallback), loading
the model once, its own, never the one kept loaded between commits
(`model.keep_loaded`), which would stop in the middle of a long run. It prints one compact trace per line, which `replay` accepts,
and takes the generation options (`--device`, `--backend`...); the model file
comes from the settings, e.g. `SMART_COMMIT_GGUF`. `prompt` and `eval` take
`--rules omitted` to leave out the fixed instructions, for a model trained
without them; otherwise the model in use decides (see
[architecture](architecture.md#prompt)). They also take `--diff plain`, the
file list without the annotations (status, renames, whitespace-only and
comment-only changes), the format the v0.2 fine-tunes were trained on, and
`--scopes map`, which shows each scope with the places its commits touched
instead of the names alone, for a model trained that way. `--examples
similar` (or `mixed`) picks the example subjects among the commits that
touched files close to the staged ones instead of the most recent, and
`--names on`, which adds a line with the functions, classes and the like the
change declares and one with the comments it adds, to evaluate those.

## Devices

```bash
smart-commit device            # the device in use and the measured times (choose another in a terminal)
smart-commit device use npu    # use the NPU from now on
smart-commit device use auto   # back to the fastest
smart-commit bench             # measure again
```

`auto` uses the fastest device of the last benchmark, and falls back to the
next one, with a note, if it becomes unavailable. See [devices](devices.md).

On `vulkan` and `cpu` the model stays loaded for 10 idle minutes after a
message, so only the first commit pays for loading it (`model.keep_loaded`;
0 turns it off, and it costs the model's memory meanwhile, which is why it is
kept only if it takes at most 15% of the RAM and leaves a tenth free, see
`model.keep_loaded_max_ram`):

```bash
smart-commit config set model.keep_loaded 30   # or 0
smart-commit server status     # what is kept loaded, and how long it has been idle
smart-commit server stop       # free its memory now
```

## Models

```bash
smart-commit model                       # choose a model (a picker in a terminal)
smart-commit model list                  # the recommended models and those from disk, and which is in use
smart-commit model use qwen2.5-coder-7b  # download it from Hugging Face and use it
smart-commit model use ~/models/my.gguf  # use a GGUF file from disk, unverified (remembered)
```

Recommended models are downloaded from Hugging Face (their publishers'
repositories, or the project's account for smart-commit's own fine-tunes),
pinned to a revision and checked against a SHA-256. See [models](models.md).

## Settings

```bash
smart-commit config list --show-origin                         # everything, and where it comes from
smart-commit config set style.default_convention conventional
smart-commit config set --repo style.convention conventional   # .smart-commit.toml, shared through git
smart-commit config init                                       # a file with every key commented out
```

| To… | Run |
|---|---|
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

A trace contains the staged diff and recent commit subjects with the paths
they touched, but not the repository's path. Review it before sharing; with
it, the case can become a test fixture.

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
smart-commit uninstall            # the binary, the models, llama.cpp, the benchmark, the caches and the config
smart-commit uninstall --system   # and the OpenVINO the setup installed (asks before using sudo)
```

lists what it removes, with the size of the models, and asks (`--yes` skips
the question; without a terminal it is required). Model files the settings
point to outside `~/.local/share/smart-commit` (`model.gguf`, `model.dir`)
were not downloaded by smart-commit: they are named and kept. It runs
`install.sh --purge` (with `--system`), the installer that came with the
release; `install.sh` keeps finer options:

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
installed, and does nothing once the binary is gone (commits are not
blocked): remove `.git/hooks/prepare-commit-msg` there, or run
`smart-commit hook uninstall` before uninstalling.

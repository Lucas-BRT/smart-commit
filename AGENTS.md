# AGENTS.md

Rules for agents, and people, who change this repository. A directory with its
own `AGENTS.md` adds to this file and never overrides it: read both.

smart-commit is a Rust CLI that writes commit messages in a repository's style
with a small language model running locally, on an Intel NPU, a GPU (Intel
through OpenVINO, AMD and NVIDIA through llama.cpp with Vulkan) or the CPU.
[docs/architecture.md](docs/architecture.md) describes how it works.

## Rules that do not bend

Each one is explained below; the last column says what catches a break.

| Rule | Caught by |
|---|---|
| A change ships with its tests, in the same commit | review |
| A commit has one purpose and passes the checks on its own | review, CI |
| Subjects are Conventional Commits in English, at most 72 columns | review, release notes |
| Never create tags or releases by hand | the release workflow |
| Fixtures and traces are generated, never edited by hand | `replay_fixtures` |
| Numbers and commands in docs and on the website come from the repository, never from memory | `docs.rs`, the website's tests |
| Anything that reaches generation is pinned: versions, hashes, decoding parameters | CI (`--locked`, hashes) |
| Stage and commit by explicit path when the index holds files that are not yours | review |

## Layout

The root is a virtual Cargo workspace; each member has its own directory.

| Directory | Package | What it is | Rules |
|---|---|---|---|
| `cli/` | `smart-commit` | The released binary (`cli/src`, `cli/tests`); the only default member | this file |
| `website/` | `smart-commit-website` | The website, built with Topcoat | this file, then [website/AGENTS.md](website/AGENTS.md) |
| `tools/` | `smart-commit-tools` | `sc-tools`: release notes, llama-server timing | this file |
| `training/tools/` | `smart-commit-training` | `sc-training`: the data tools for the models (the rest of `training/` is Python, see [training/README.md](training/README.md)) | this file |

`docs/`, `scripts/`, `install.sh` and the pinned toolchain belong to the whole
repository and stay at the root. `cargo build` makes the CLI only;
`cargo build --profile website -p smart-commit-website` makes the site.
Dependencies and package fields that several members share are declared once
in the root `Cargo.toml` (`[workspace.dependencies]`, `[workspace.package]`).

## Checks

CI runs all of these on every push, and they must pass before any commit:

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
shellcheck install.sh scripts/*.sh cli/tests/fixtures/make-traces.sh cli/tests/scripts/*.sh   # 0.11.0, as CI
for test in cli/tests/scripts/test-*.sh; do "$test"; done   # test-install.sh needs Docker
for test in training/test_*.py; do python3 -I "$test"; done  # test_train_dpo.py needs torch
```

A change to the website adds its own check, which tests cannot replace; see
[website/AGENTS.md](website/AGENTS.md).

## Tests come with every change

**Every change ships with its tests, in the same commit.** There is no "tests
later".

- **New behaviour:** tests for the normal case and the edges (empty input,
  limits, errors).
- **Bug fix:** a regression test that fails without the fix. Check that it
  does: restore the old code, run it, put the fix back.
- **Refactoring:** the existing tests keep passing unchanged; if one must
  change, the commit message says why.

| Kind | Where |
|---|---|
| Unit | `tests.rs` beside the `mod.rs` of a module with its own directory, or `#[cfg(test)] mod tests` in a small file, under `cli/src/` |
| Prompt snapshots | `insta::assert_snapshot!`, files in `snapshots/` beside the module; update with `INSTA_UPDATE=always cargo test` and review the diff |
| Git integration | `cli/tests/git_context.rs`, with `TestRepo` from `cli/tests/common/` |
| CLI end to end | `cli/tests/{cli,config_cli,bench_cli,hook,setup_cli}.rs`, running the binary (`CARGO_BIN_EXE_smart-commit`) |
| Trace fixtures | `cli/tests/replay_fixtures.rs` |
| Docs | `cli/tests/docs.rs`: the paths the pages name exist, every command is in `docs/usage.md` |
| Tools | inside `tools/` and `training/tools/`; the scenarios built twice need `cargo test -- --ignored` after `cargo build` |
| Scripts | `cli/tests/scripts/test-*.sh`, against fake sysfs trees, releases and binaries; no network, sudo or hardware |
| Python | `training/test_*.py`, run with `python3 -I` |
| Website | `website/tests/routes.rs`: every page, and the facts it takes from the product |
| Hardware | `cli/tests/hardware.rs`, ignored by default: `cargo test --test hardware -- --ignored --test-threads 1`, with `SMART_COMMIT_TEST_DEVICE=npu\|intel-gpu\|vulkan\|cpu\|openvino-cpu`; use the fixture prompts, never ad hoc inputs |

`cargo test` never depends on an NPU or GPU, the network, the clock,
randomness, or the machine's git config, smart-commit config, models and
`SMART_COMMIT_*` variables: run commands through `cli/tests/common::isolate`.
Nothing that reaches the output may depend on `HashMap` iteration order.

## Commits

### One purpose per commit

Each commit has one purpose (a feature, a fix, a refactoring, a docs change),
and the project builds and passes its checks at every commit. When a file has
changes for two purposes, stage the hunks of one: `git add -p FILE`, or, with
no terminal, edit a partial patch and `git apply --cached` it. Then look at
`git diff --staged` (what goes in) and `git diff` (what is left).

Check that **what is staged alone** builds and passes, in a throwaway worktree
(never `git stash`, which is shared with other sessions):

```bash
git worktree add --detach /tmp/staged && git diff --staged | git -C /tmp/staged apply
(cd /tmp/staged && cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace)
git worktree remove --force /tmp/staged
```

### Commit by explicit path

When the index holds files that are not part of your change (a teammate's
staged file, a local `mise.toml`), commit only yours, and never unstage or
include the others:

```bash
git commit -F message.txt -- path/one path/two
```

### Ask smart-commit first

Before each commit, with only that commit's changes staged, run the tool on
its own change and keep the trace:

```bash
smart-commit --trace /tmp/trace.json                      # the installed release
target/release/smart-commit --trace /tmp/trace-dev.json   # this tree
```

Compare its message with the one you write. A wrong one (type, scope, invented
details, a copied history subject, a fallback) is a bug found in real use: turn
it into a fixture in `cli/tests/fixtures/traces/` (via
`cli/tests/fixtures/make-traces.sh`), in a commit of its own. The trace is what
makes it reproducible; a bad message without one cannot be investigated.

### Messages

Conventional Commits in English (`feat: ...`, `fix(diff): ...`, `docs: ...`,
`test: ...`), with a subject of at most 72 columns, and `!` plus a `Breaking:`
paragraph for breaking changes. The body explains why, when that is not
obvious. Release notes are generated from these subjects.

## Reproducibility

Any result (a build, a test, a generated message) must be reproducible by
someone else on another machine. [docs/reproducibility.md](docs/reproducibility.md)
says what is pinned and how it is checked; what an agent must remember:

- **Builds:** `rust-toolchain.toml` pins the toolchain, `Cargo.lock` is
  versioned, CI uses `--locked`, releases are built in the container of
  `scripts/build/Dockerfile`. Dependency updates get commits of their own.
- **Generation:** greedy decoding with explicit parameters (`GenerationConfig`
  and what each backend sends), never library defaults. A retry must change
  the prompt deterministically.
- **Pinned downloads:** drivers, runtimes, llama.cpp and models are pinned by
  version and SHA-256 in the scripts; change the version and the hash together.
- **Inputs are versioned:** fixtures live in `cli/tests/fixtures/` or inline in
  tests, never at paths of the author's machine.
- **Traces:** a bad message found in real use becomes a fixture in
  `cli/tests/fixtures/traces/`. A change that alters the pipeline on purpose
  regenerates them with `cli/tests/fixtures/make-traces.sh` in the same commit,
  and their diff is reviewed. Fixtures are never edited by hand.
- **Model export:** a change to the export (revision, flags, lock) needs two
  identical exports from scratch, the new `scripts/export/MANIFEST.sha256`
  committed with it, and the **Model** workflow run, which checks the export on
  another machine.
- **Where models come from:** other publishers' models are never re-hosted:
  users' machines download them from their publishers and export them. The
  project's own fine-tunes (`smart-commit-<size>-v<version>`) live on Hugging
  Face under [LucasBRT](https://huggingface.co/LucasBRT), one repository per
  model and version, with a model card. A new version is a new name, never a
  replaced file, pinned in `cli/src/models/registry.rs` by revision and
  SHA-256. [docs/models.md](docs/models.md) has the full policy.

## Releases

- **To release, bump `version` in `cli/Cargo.toml`** (semver) in a commit
  `chore: release X.Y.Z` on `main`. `.github/workflows/release.yml` tests,
  packages and publishes the release, with notes from the commits
  (`sc-tools release-notes`). Never create tags or releases by hand.
- The tarball must stay reproducible: `cli/tests/scripts/test-install.sh`
  packages twice and requires identical bytes, and CI prints the hash of every
  commit.

## Configuration

Every setting is declared in `config::KEYS` (type, scope, environment variable,
description), never read straight from a file or the environment elsewhere.
Keys about the machine are `Scope::User` (global only). A setting that changes
what is generated must reach traces through `PromptOptions` or
`GenerationConfig`. [docs/configuration.md](docs/configuration.md) must list
every key; a test checks it.

## Documentation

Docs describe the current state, in English. A change updates the pages it
affects in the same commit (usage, configuration, devices, architecture,
reproducibility); [docs/roadmap.md](docs/roadmap.md) tracks what is done and
what is next. Numbers (timings, counts) come from measurements that can be
repeated, never from memory. Timings live in
[docs/measurements.md](docs/measurements.md) only: other pages link to it.

The website reads its version, repository, install command, default model,
devices table, benchmark example and next roadmap items from the files that
say them (`cli/Cargo.toml`, `cli/src/models/registry.rs`, `docs/devices.md`,
`docs/usage.md`, `docs/roadmap.md`). Change the source, not the site; if you
change the shape of one of those files, `website/build.rs` stops the build and
says which.

## Before you say it is done

1. The checks above pass, on what you committed, and you ran them rather than
   assumed.
2. Every change has its test, and a bug fix has one that fails without it.
3. Docs, roadmap and the website's facts follow the change.
4. In your report, say what you did not check or could not run.

## Changing this file

Keep it to what an agent cannot learn from the code or the docs. When the same
correction is made twice, add the rule here, with what catches a break; when CI
or a test enforces a rule, shorten it to a line; when a rule has not mattered
in a long time, delete it. A rule that only one directory needs goes in that
directory's `AGENTS.md`.

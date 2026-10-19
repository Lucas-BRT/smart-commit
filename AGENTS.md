# AGENTS.md

Rules for agents, and people, who change this repository.

## Project

smart-commit is a Rust CLI that writes commit messages in a repository's
style with a small language model running locally, on an Intel NPU, a GPU
(Intel through OpenVINO, AMD and NVIDIA through llama.cpp with Vulkan) or the
CPU. [docs/architecture.md](docs/architecture.md) describes how it works.

## Checks

CI runs all of these on every push, and they must pass before any commit:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
shellcheck install.sh scripts/*.sh tests/fixtures/make-traces.sh tests/scripts/*.sh
for test in tests/scripts/test-*.sh; do "$test"; done   # test-install.sh needs Docker
```

## Tests come with every change

**Every change ships with its tests, in the same commit.** There is no
"tests later".

- **New behaviour:** tests for the normal case and the edges (empty input,
  limits, errors).
- **Bug fix:** a regression test that fails without the fix.
- **Refactoring:** the existing tests keep passing unchanged; if one must
  change, the commit message says why.

| Kind | Where |
|---|---|
| Unit | `#[cfg(test)] mod tests` in the module, under `src/` |
| Prompt snapshots | `insta::assert_snapshot!`, files in `src/snapshots/`; update with `INSTA_UPDATE=always cargo test` and review the diff |
| Git integration | `tests/git_context.rs`, with `TestRepo` from `tests/common/` |
| CLI end to end | `tests/cli.rs`, `tests/config_cli.rs`, `tests/bench_cli.rs`, `tests/hook.rs`, running the binary (`CARGO_BIN_EXE_smart-commit`) |
| Trace fixtures | `tests/replay_fixtures.rs` |
| Scripts | `tests/scripts/test-*.sh`, against fake sysfs trees, releases and binaries; no network, sudo or hardware |
| Hardware | `tests/hardware.rs`, ignored by default: `cargo test --test hardware -- --ignored --test-threads 1`, with `SMART_COMMIT_TEST_DEVICE=npu\|intel-gpu\|vulkan\|cpu\|openvino-cpu`; use the fixture prompts, never ad hoc inputs |

`cargo test` never depends on an NPU or GPU, the network, the clock,
randomness, or the machine's git config, smart-commit config, models and
`SMART_COMMIT_*` variables: run commands through `tests/common::isolate`.
Nothing that reaches the output may depend on `HashMap` iteration order.

## Reproducibility

Any result (a build, a test, a generated message) must be reproducible by
someone else on another machine. [docs/reproducibility.md](docs/reproducibility.md)
describes what is pinned and how it is checked.

- **Builds:** the toolchain is pinned in `rust-toolchain.toml`, `Cargo.lock`
  is versioned, CI uses `--locked`, and releases are built in the pinned
  container of `scripts/build/Dockerfile`. Dependency updates get commits of
  their own.
- **Generation:** greedy decoding with explicit parameters
  (`GenerationConfig` and what each backend sends), never library defaults.
  A retry must change the prompt deterministically.
- **Pinned downloads:** drivers, runtimes, llama.cpp and models are pinned by
  version and SHA-256 in the scripts; change the version and the hash
  together.
- **Inputs are versioned:** fixtures live in `tests/fixtures/` or inline in
  tests, never at paths of the author's machine.
- **Traces:** a bad message found in real use becomes a fixture in
  `tests/fixtures/traces/`. Changes that alter the pipeline on purpose
  regenerate the fixtures with `tests/fixtures/make-traces.sh` in the same
  commit, and their diff is reviewed. Fixtures are never edited by hand.
- **Model export:** a change to the export (revision, flags, lock) needs two
  identical exports from scratch, the new `scripts/export/MANIFEST.sha256`
  committed with it, and the **Model** workflow run, which checks the export
  on another machine.
- **Where models come from:** other publishers' models are never
  re-hosted: users' machines download them from their publishers and
  export them. The project's own fine-tunes (`smart-commit-<size>-v<N>`,
  e.g. `smart-commit-1.5b-v1`) are published on Hugging Face under the
  project's account: the merged weights in safetensors and the Q4_K_M GGUF,
  each version in a commit of that repository with a model card (base
  model, data and its licence, prompt format, evaluation against the base,
  limitations). Their OpenVINO export still runs on users' machines, from
  those safetensors. A new version is a new name (`-v2`), never a
  replaced file, and is pinned in `src/models.rs` by revision and SHA-256
  like any other model.

## Commits

### One purpose per commit

Each commit has one purpose (a feature, a fix, a refactoring, a docs change),
and the project builds and passes its checks at every commit.

### Same file, different purposes: stage hunks

When a file has changes for different purposes, for example a bug fix and a
feature in the same `src/diff.rs`, **do not stage the whole file**. Stage the
hunks of one purpose, commit, and repeat:

```bash
git add -p src/diff.rs        # y = stage, n = skip, s = split, e = edit
git diff --staged             # what goes into this commit
git diff                      # what is left for the next one
```

Without an interactive terminal (agents, scripts), do the same with a partial
patch:

```bash
git diff src/diff.rs > /tmp/partial.patch
# edit the patch, keeping only this purpose's hunks
git apply --cached /tmp/partial.patch
git diff --staged
```

Before each commit, check that **what is staged alone** builds and passes:

```bash
git stash push --keep-index --include-untracked
cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
git stash pop
```

### Ask smart-commit first

Before each commit, with only that commit's changes staged, run the tool
on its own change and keep the trace:

```bash
smart-commit --trace /tmp/trace.json            # the installed release
target/release/smart-commit --trace /tmp/trace-dev.json   # this tree
```

Compare its message with the one you write. A wrong one (type, scope,
invented details, a copied history subject, a fallback) is a bug found in
real use: turn it into a fixture in `tests/fixtures/traces/` (via
`tests/fixtures/make-traces.sh`), in a commit of its own. The trace is what
makes it reproducible; a bad message without one cannot be investigated.

### Messages

Conventional Commits in English (`feat: ...`, `fix(diff): ...`, `docs: ...`,
`test: ...`), with a subject of at most 72 columns, and `!` plus a
`Breaking:` paragraph for breaking changes. The body explains why, when that
is not obvious. Release notes are generated from these subjects.

## Releases

- **To release, bump `version` in `Cargo.toml`** (semver) in a commit
  `chore: release X.Y.Z` on `main`. `.github/workflows/release.yml` tests,
  packages and publishes the release, with notes from the commits
  (`scripts/release-notes.sh`). Never create tags or releases by hand.
- The tarball must stay reproducible: `tests/scripts/test-install.sh`
  packages twice and requires identical bytes, and CI prints the hash of
  every commit.

## Configuration

Every setting is declared in `config::KEYS` (type, scope, environment
variable, description), never read straight from a file or the environment
elsewhere. Keys about the machine are `Scope::User` (global only). A setting
that changes what is generated must reach traces through `PromptOptions` or
`GenerationConfig`. [docs/configuration.md](docs/configuration.md) must list
every key; a test checks it.

## Documentation

Docs describe the current state, in English. A change updates the pages it
affects in the same commit (usage, configuration, devices, architecture,
reproducibility); [docs/roadmap.md](docs/roadmap.md) tracks what is done and
what is next. Numbers in the docs (timings, counts) come from measurements
that can be repeated, never from memory.

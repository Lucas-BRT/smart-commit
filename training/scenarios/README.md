# Commit scenarios

Hand-written cases of git operations, from a one-character typo to a merge
in progress, each with the message a good tool would write. They complement
CommitChronicle, which has no merges, reverts, renames of directories,
mode changes or Portuguese, and whose targets were written by whoever made
the commit.

A case is data: a base repository, a list of operations, a target. Building
it runs smart-commit's mock backend on the result and keeps the trace's
inputs, so the cases come out in the format of `sc-training select`:

```bash
SC=../target/release/smart-commit
SCT=../target/release/sc-training   # cargo build --locked --release -p smart-commit-training
$SCT scenarios list                      # cases per level and split
$SCT scenarios build data/scenarios-eval $SC --split eval
$SCT scenarios build data/scenarios-train $SC --split train

# Evaluate, like eval-v1: one section per level besides free/conventional.
XDG_CONFIG_HOME=$C/empty-config SMART_COMMIT_GGUF=... \
  $SC eval data/scenarios-eval/inputs.jsonl --device vulkan > data/scenarios-eval/h1.jsonl
$SCT score data/scenarios-eval data/scenarios-eval/h1.jsonl

# Train: examples like any selection, then appended to the run's examples.
$SCT make-examples data/scenarios-train $SC
cat data/h1/examples.jsonl data/scenarios-train/examples.jsonl > data/h1s/examples.jsonl
```

`cargo test --locked -p smart-commit-training` checks the cases and the
builder; with `-- --ignored` (and `cargo build` first) it also builds everything
twice, requiring identical files.

## Train or eval

Every case has a `split`. A case in `train` must never be in `eval`, or the
score measures memory. Variants of one case (the same operation on other
files) can be split between them. Cases whose target depends on what the
trace inputs do not record (`git-state`: a merge or revert in progress, a
deletion left unstaged; `file-mode`: `chmod`, symlinks) are eval only: a
model trained on them would learn to guess.

## Levels

| Level | File | What |
|---|---|---|
| 0 | `cases/0-metadata.toml` | little or no code meaning: typos, whitespace, line endings, modes |
| 1 | `cases/1-small.toml` | one file, one to five lines |
| 2 | `cases/2-single-file.toml` | one file, ten to a hundred lines |
| 3 | `cases/3-related-files.toml` | a few related files, one purpose |
| 4 | `cases/4-large.toml` | large or mechanical, beyond what the prompt shows |
| 5 | `cases/5-git-state.toml` | git states and unusual situations |

## Base repositories

`repos/<name>/files/` is the final tree; `repos/<name>/repo.toml` names the
convention and the history that introduces it, oldest first. Each commit
adds the `paths` it lists from `files/` and applies its `ops`; every file
must be introduced by some commit. Author, dates and git config are fixed,
so the hashes are the same on every machine.

| Repo | Language | Style |
|---|---|---|
| `cafeteria` | Python, Portuguese identifiers | free, Portuguese, informal |
| `ledger` | TypeScript | Conventional Commits with scopes, English |

`sc-training scenarios show DIR --repo cafeteria` leaves a base repository in
DIR, and `show DIR --case ID` a case applied on it, to look at while
writing cases.

## Writing a case

```toml
[[case]]
id = "rename-file"                    # unique; variants get id/name
title = "rename puro"
repo = "cafeteria"                    # or set once at the top of the file
split = "train"                       # train or eval
ops = [
  { op = "git", args = ["mv", "src/cafeteria/{{old}}.py", "src/cafeteria/{{new}}.py"] },
  { op = "regex", paths = ["src/cafeteria/*.py"], pattern = "cafeteria\\.{{old}}\\b", repl = "cafeteria.{{new}}", optional = true },
]
target = "renomeia {{old}}.py para {{new}}.py"   # in the repository's style
expect = ["renome|move", "{{new}}"]   # every pattern must match a right subject
tags = ["rename"]

  [[case.variants]]                   # optional: one case per variant
  name = "relatorio"
  vars = { old = "relatorio", new = "relatorios" }

  [[case.variants]]
  name = "estoque"
  split = "eval"                      # a variant may override split, target, expect
  vars = { old = "estoque", new = "armazem" }
```

The top of a case file can set `level`, `repo` and `split` for all its
cases. Other fields: `stage` (`all`, the default, runs `git add -A` after
the operations; `none` leaves staging to them), `history = false` (the
files with no commit, for a first commit), `outcome = "error"` (smart-commit
must refuse, e.g. nothing staged) and `issue` (a known bug: the build
reports the case instead of failing).

`expect` patterns are regular expressions matched on the subject in
lowercase without accents; `sc-training score` reports them as `expected`. `{{name}}`
is replaced only in cases with variants, so code keeps its own braces.

## Operations

| op | Fields | Does |
|---|---|---|
| `write` | `path`, `text`, [`repeat`, `join`] | creates or replaces a file; with `repeat`, that many copies of `text` with `{{i}}` counting from 0 |
| `append`, `prepend` | `path` or `paths` (globs), `text`, [`repeat`] | adds text at the end or the start |
| `replace` | `path(s)`, `old`, `new`, [`all`] | literal; `old` must occur exactly once unless `all` |
| `regex` | `path(s)`, `pattern`, `repl`, [`optional`] | `re.sub`, multiline; fails on no match unless `optional` |
| `remove` | `path` | deletes a file or directory, unstaged |
| `move` | `from`, `to` | renames on disk, without git |
| `chmod` | `path`, `mode` | e.g. `"755"` |
| `symlink` | `path`, `target` | |
| `binary` | `path`, `size`, [`seed`] | seeded random bytes |
| `git` | `args`, [`check`] | any git command; `{{commit:subject}}` is the hash of the commit with that subject; `check = false` tolerates a failure (a merge conflict) |

A bad message found in real use that a case can reproduce becomes a case
here, as well as a trace fixture in `cli/tests/fixtures/traces/`.

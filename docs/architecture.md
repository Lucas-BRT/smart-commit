# Architecture

```
git log ──────────┐
branch ───────────┼─► style ─┐
git diff --staged ┘          ├─► prompt ─► backend ─► sanitize ─┬─► message
                   diff ─────┘   (fits the     (device)          │
                   (compacted)    NPU's limit)                   └─ rejected: retry once,
                                                                    then a fallback
```

Everything except the model is deterministic and tested without hardware.

## Modules

| Module | Responsibility |
|---|---|
| `git` | `Repo`: runs the `git` binary (so the user's config applies) with machine-readable output (`-z`, `--format`) |
| `style` | infers convention, language, types, scopes, ticket style and examples from commit subjects |
| `context` | `RepoContext`: branch, ticket, subjects, style and staged files, rebuildable from recorded data |
| `diff` | parses `--numstat -z` and the patch, classifies files, fits them into a token budget |
| `prompt` | builds the system and user messages within `max_prompt_tokens` |
| `message` | sanitizes the model's output; builds the fallback message |
| `generate` | prompt → model → sanitize → retry → fallback, keeping every attempt |
| `backend` | the `Backend` trait, `GenerationConfig`, `MockBackend` |
| `genai` | OpenVINO GenAI backend (`npu`, `intel-gpu`, `openvino-cpu`), loaded at run time |
| `llamacpp` | llama.cpp backend (`vulkan`, `cpu`): a private `llama-server` over a UNIX socket |
| `device` | candidate devices and loading the first that works |
| `bench` | measures every device on the embedded fixture prompts |
| `models` | the recommended models, pinned on Hugging Face, and installing them |
| `config` | settings from flags, environment, repository and global files |
| `trace` | records and replays generations |
| `hook` | the `prepare-commit-msg` script and how it fills the message file |
| `paths` | XDG locations of config, models, caches and the benchmark |

The binary (`src/main.rs`, `config_cmd.rs`, `bench_cmd.rs`) only parses
arguments and wires these together.

## Repository style

From the last `style.history_depth` (30) subjects, merges excluded:

- **Convention:** Conventional Commits (`type(scope)!: description`) or
  gitmoji (an emoji or `:shortcode:` first) when at least 60% of the subjects
  follow it; otherwise `Free`; `Unknown` without history.
- **Language:** counts common Portuguese and English words in the
  description (after the `type(scope):` or emoji prefix, so a `fix:` does not
  count as English). It needs at least 2 hits, and a tie is `Unknown`.
- **Types and scopes:** ranked by use; the prompt lists the 8 most used.
- **Tickets:** an uppercase Jira-style key (`ABC-123`) in the branch; it goes
  into the prompt only if at least 50% of the subjects cite one, at the start
  (also right after the convention prefix) or elsewhere.
- **Examples:** up to 5 recent, distinct subjects of 8 to 100 characters
  that follow the convention. `fixup!`, `squash!`, `amend!`, reverts and WIP
  are left out.

## Fitting the diff

The NPU has a fixed prompt length, so the diff is reduced to a budget, in
tokens estimated as characters / 3 (pessimistic: real prompts are 3.4–3.75
characters per token, and overflowing would fail the generation).

1. The patch has one line of context (`-U1`), without external diff tools
   or colours. Patch sections are paired with `--numstat` entries by order;
   if they disagree, only the file list is kept.
2. Files are classified by path: source, config, tests, docs, and noise
   (lockfiles, generated files, binaries), which only appear in the file list.
3. The file list comes first and may use half of the budget; what does not
   fit is summarised as `... and N more files`.
4. The rest is shared by priority (source, config, tests, docs): each file
   of a tier gets an equal share, smaller files give back what they do not
   need, and leftovers go to the next tier. A file whose share is under 24
   tokens is left out rather than cut to a few misleading lines; cut files
   end with `[... N more lines]`.

## Prompt

The instructions are in English, which small models follow best, and ask for
the repository's language. They state the 72-column subject limit, the
convention with its types and scopes, the ticket when the repository cites
them, and the history examples. The user message has the file list, the diff
and, when files were cut, a note saying so. A forced convention
(`style.convention`) drops the examples that contradict it.

## Sanitizing the output

Small models wrap answers in code fences, quote them, announce them ("Here is
the commit message:"), explain them afterwards, emit `<think>` blocks or
overflow 72 columns. In order, the sanitizer:

- drops `<think>…</think>` (an unclosed block swallows the rest);
- keeps the first code fence's contents;
- skips preambles and inline labels (`Commit message:`), list markers,
  headings, quotes, backticks and bold around the subject; collapses spaces;
  drops the final period;
- cuts explanations after the message ("This commit message…");
- cuts the subject at the last word that fits in 72 columns, and rewraps the
  body at 72, keeping paragraphs and bullets;
- for Conventional Commits, fixes near misses (`Feat: x`, `feat:x`) and
  rejects anything else.

## Retry and fallback

A rejected reply (empty, or not conventional when it must be) gets **one
retry**, whose prompt ends with why the first one was rejected. With greedy
decoding, repeating the same prompt would give the same reply. If the retry
is rejected too, the message is built from the file list alone (for example
`chore: update 3 files in src/`, with the files in the body), in the
repository's language and convention, and a warning says so. Backend errors
are reported, never hidden by the fallback.

## Git hook

`smart-commit hook install` writes `prepare-commit-msg`, honouring
`core.hooksPath`. The script calls the binary by absolute path, because GUI
clients often run hooks without the user's `PATH`.

- It only acts on a plain `git commit`; with `-m`, `-F`, a template, a merge,
  a squash or an amend, git already has a message.
- It never overwrites a message already in the file. With `git commit -v`,
  the diff after the scissors line does not count.
- It sees exactly what will be committed: git runs hooks with
  `GIT_INDEX_FILE` set, for example to the temporary index of `commit -a`.
- **It never blocks a commit.** Any error becomes a warning, and the commit
  goes on as if the hook were not there.
- It never touches hooks it did not install: `--force` keeps them as a
  backup, without ever overwriting an earlier backup, and `uninstall`
  restores them.

## Backends

- **OpenVINO GenAI** is loaded at run time (`runtime-linking`), so the binary
  builds and runs without it and says how to install it. The binding's errors
  carry no message, so everything the user can fix (missing model, missing
  library) is checked first. Every generation parameter is explicit (maximum
  tokens, no sampling, one beam, no repetition penalty), never taken from the
  model's `generation_config.json`.
- **llama.cpp** runs a private `llama-server` per session, listening on a
  UNIX socket in a fresh `0700` temporary directory, so nothing is exposed on
  the network. A minimal HTTP/1.1 client talks to it, and requests are greedy
  and seeded, without prompt caching. The server stops when the backend is
  dropped. On `cpu` it offloads nothing (`--n-gpu-layers 0 --device none`),
  even when it finds a GPU. With fast load (`--no-repack --fit off`, where
  the benchmark found it faster) a failed start is retried without it.

See [devices](devices.md) for how the device is chosen and how they compare.

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
| `annotate` | the annotated file list: status, renames as `dir/{old => new}`, whitespace-only and comment-only changes |
| `prompt` | builds the system and user messages within `max_prompt_tokens` |
| `message` | sanitizes the model's output; builds the fallback message |
| `generate` | prompt → model → sanitize → retry → fallback, keeping every attempt |
| `backend` | the `Backend` trait, `GenerationConfig`, `MockBackend` |
| `genai` | OpenVINO GenAI backend (`npu`, `intel-gpu`, `openvino-cpu`), loaded at run time |
| `llamacpp` | llama.cpp backend (`vulkan`, `cpu`): a private `llama-server` over a UNIX socket |
| `device` | candidate devices and loading the first that works |
| `bench` | measures every device on four embedded prompts sized like real ones |
| `hardware` | the CPU, GPUs and NPU from sysfs, and the devices each gives |
| `setup` | the steps the hardware needs, for the setup screen |
| `models` | the recommended models, pinned on Hugging Face, and installing them |
| `tui` | the full-screen screens (ratatui), shown only when stdin and stdout are terminals |
| `config` | settings from flags, environment, repository and global files |
| `trace` | records and replays generations |
| `hook` | the `prepare-commit-msg` script and how it fills the message file |
| `paths` | XDG locations of config, models, caches and the benchmark |

The binary (`src/main.rs`, `config_cmd.rs`, `bench_cmd.rs`, `model_cmd.rs`,
`setup_cmd.rs`) only parses arguments and wires these together.

## Repository style

From the last `style.history_depth` (30) subjects, merges excluded:

- **Convention:** Conventional Commits (`type(scope)!: description`) or
  gitmoji (an emoji or `:shortcode:` first) when at least 60% of the subjects
  follow it; otherwise `Free`; `Unknown` without history.
- **Language:** counts common Portuguese and English words in the
  description (after the `type(scope):` or emoji prefix, so a `fix:` does not
  count as English). It needs at least 2 hits, and a tie is `Unknown`.
- **Types and scopes:** ranked by use; the prompt lists the 8 most used.
  The paths each history commit touched are recorded too (traces keep
  them), for the evaluation's `scope_fits` and for a model trained to use
  them (training H4b); the prompt does not show them, since alone they did
  not help the models.
- **Tickets:** an uppercase Jira-style key (`ABC-123`) in the branch; it goes
  into the prompt only if at least 50% of the subjects cite one, at the start
  (also right after the convention prefix) or elsewhere.
- **Examples:** up to 5 recent, distinct subjects of 8 to 100 characters
  that follow the convention. `fixup!`, `squash!`, `amend!`, reverts and WIP
  are left out.

## Fitting the diff

The NPU has a fixed prompt length, so the diff is reduced to a budget, in
tokens estimated as characters / 3. That is pessimistic for most prompts
(3.4–3.75 characters per token), not for all: hashes, base64 and long
numbers take more tokens (Qwen's tokenizer splits every digit), and on
eval-v1 the estimate was up to 1.45 times short for 2–3% of the prompts.
llama.cpp starts its server with half the budget again as room, so such a
prompt is read a little longer instead of failing.

1. The patch has one line of context (`-U1`), without external diff tools
   or colours. Patch sections are paired with `--numstat` entries by order;
   if they disagree, only the file list is kept.
2. Files are classified by path: source, config, tests, docs, and noise
   (lockfiles, generated files, binaries), which only appear in the file list.
3. The file list comes first and may use half of the budget; what does not
   fit is summarised as `... and N more files`.
4. The rest is shared by priority (source, config, tests, docs): each file
   of a tier gets an equal share, smaller files give back what they do not
   need, and leftovers go to the next tier. A cut file shows at least its
   headers and three of its lines (and 24 tokens): when the equal share is
   less, the first files of the tier get that much while the budget lasts
   and the others appear in the file list only, so a section is never a
   bare header. Cut files end with `[... N more lines]`.

## Prompt

The instructions are in English, which small models follow best, and ask for
the repository's language. They state the 72-column subject limit, the
convention with its types and scopes, the ticket when the repository cites
them, and the history examples. The user message has the file list, the diff
and, when files were cut, a note saying so. A forced convention
(`style.convention`) drops the examples that contradict it.

The file list is plain (`path | +3 -1`), the format the fine-tunes were
trained on. `--diff annotated` (`prompt` and `eval` only, recorded in
traces) adds what the counts do not say: `new`, `deleted`, `renamed` with
git's short form `dir/{old => new}/file` and one line for three or more
files moved between the same directories, `copied from`, `empty`, `no line
changed (mode)`, `whitespace only` with which whitespace (`CRLF to LF`,
`final newline`, `trailing spaces`, `indentation`, `blank lines`: the
file leaves the diff and its budget, since the change is invisible there;
`line breaks`, code reformatted, stays in the diff), `comments only` (comments, docstrings, blank lines, by extension),
and `maybe renamed from` for a new file sharing 40% of its lines with a
deleted one, below git's rename threshold.

A model trained to follow the fixed instructions without being told
(training H6) gets a shorter system message, `PromptRules::Omitted`: only
what varies by repository (types, scopes, examples) and what its training
lacks (a ticket, Portuguese, gitmoji). The rules follow the model file that
runs, from its entry in `models::REGISTRY` and the checked SHA-256 next to
it, and are recorded in traces; any other file gets every rule.

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

A rejected reply gets **one retry**, whose prompt ends with why the first
one was rejected. A reply is rejected when it is empty, not conventional
when it must be, a subject repeated from the history (as small models do
when the history is short or the diff says little), or nearly so (every
word of its description, four or more, is in one history subject and
covers 60% of it; a reply with words of its own is kept), or clearly in
another language than the repository's (all its known words English in a
Portuguese repository, or the reverse; a mix is kept, as such
repositories often write; a language that is only the default, with no
history to tell, is not enforced). With greedy
decoding, repeating the same prompt would give the same reply. If the retry
is rejected too, the message is built from the file list (for example
`chore: update 3 files in src/`, with the files in the body), in the
repository's language and convention, and a warning says so. When the last
reply was rejected for what it said (a copy, another language) rather than
its form, the fallback keeps its scope, and its type unless the files tell
one (all docs, tests or config): models often pick the right type even
when they copy, and the file list's guess for code is `chore`. Backend errors
are reported, never hidden by the fallback.

## Lockfiles and generated files only

When every staged file was written by a tool, the prompt would show the
model nothing but their names (their diffs are left out as noise), and
small models then tend to repeat the history. The message is then built
from the file list (`chore: add Cargo.lock`) **without loading the model**,
which also makes it instant. The evidence must be strong:

- a known lockfile name (`Cargo.lock`, `package-lock.json`, `go.sum`…) or
  the `.lock` extension;
- a generator's marker (`@generated`, `DO NOT EDIT`) in the first lines of
  the new file, as Cargo, Go, protoc, sqlc and OpenAPI generators write;
- `linguist-generated` in `.gitattributes`, read from the index.

Directory guesses such as `build/` or `vendor/` are not enough: they only
hide the diff from the prompt. A lockfile the list does not know goes to
the model like any file. Asking for another message in the review asks
the model.

## Merges, reverts and cherry-picks

A merge, a revert or a cherry-pick in progress leaves git's own message in
`MERGE_MSG` (`Merge branch 'x'`, `Revert "..."` with the reverted hash,
the picked commit's message), which `git commit` would use. The diff does
not show what is going on, so a model describes it as an ordinary change;
smart-commit takes git's message instead, comments removed, **without
loading the model**. A merge resolved to what HEAD already has stages
nothing; that is not "nothing staged" then. `git merge --squash` writes
`SQUASH_MSG`, a list of the squashed commits rather than a subject: the
model describes the change. The message is in the trace inputs
(`prepared`), so `eval` and `replay` reproduce it. Asking for another
message in the review asks the model.

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
  dropped, unless `model.keep_loaded` asks to keep it: then a detached
  supervisor (`smart-commit keep-server`, in its own process group) owns it
  and stops it after that many idle minutes, and commits connect to its
  socket in `$XDG_RUNTIME_DIR/smart-commit/<key>/`. The key is a hash of the
  server's options and of the model's and server's identity, so another
  model or device gets its own server; a lock keeps two commits from
  starting two. On `cpu` it offloads nothing (`--n-gpu-layers 0 --device none`),
  even when it finds a GPU. With fast load (`--no-repack --fit off`, where
  the benchmark found it faster) a failed start is retried without it.

See [devices](devices.md) for how the device is chosen and how they compare.

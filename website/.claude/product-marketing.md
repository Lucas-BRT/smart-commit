# Product marketing context: smart-commit

## What it is
A support tool for naming git commits. It reads the staged diff and the repository's
own history, and writes one commit message in that repository's style. It does not
replace git, a git client or the developer's judgment: the developer reads the
message, edits it, and commits.

## Who it is for
Developers who write code by hand on purpose and distrust AI that does the thinking
for them. They still find naming commits tedious. They do not want an account, an API
key, a subscription or a model that sends their diffs somewhere.

## Problem
Naming a commit is a small, repeated chore at the end of real work. Existing AI tools
either cost money per token, need the cloud, or pull the developer into a workflow
where the AI writes the code too.

## Position
AI that is small, useful and affordable, not a large cost that erodes your own
ability to think. A 1.1 GB model on the hardware you already have. 100% offline.
The tool stays out of the way: one command, one line, you decide.

## Differentiators (all stated in the project docs)
- Entirely local: no account, no API key, nothing leaves the machine.
- Follows the repository's style: convention, language, scopes, ticket format.
- Runs on what you have: Intel NPU, Intel GPU, any GPU through Vulkan, or the CPU.
- Works with any git client through a `prepare-commit-msg` hook; never blocks a commit.
- Reproducible: toolchain, dependencies and models pinned by version and hash.

## Proof (docs/measurements.md only; never invent numbers)
- Default model is 1.1 GB.
- Per message, end to end: 1.8 s on an Intel iGPU, 2.4 s on an RTX 3050 through
  Vulkan, 4.0 s on a Ryzen 5 5500 CPU.

## Primary action
Install it: one `curl` command, no sudo, installs in `~/.local`.

## Voice
Plain, calm, a little dry. No hype, no "supercharge", no exclamation marks. Honest
about limits (Linux x86_64 only today; the model suggests, the developer decides).

## Objections
- "I do not want AI in my workflow": it writes one line you review; it does not write code.
- "Local models are slow or need a big GPU": it runs on the CPU; numbers above.
- "Will it lock me in": it is a hook and a CLI; remove it and git is untouched.

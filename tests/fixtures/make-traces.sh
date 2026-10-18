#!/usr/bin/env bash
# Regenerates tests/fixtures/traces from deterministic throwaway
# repositories (fixed identity, dates and config), so the output is the same
# on every machine. Run from the repository root:
#
#   tests/fixtures/make-traces.sh
#
# then review the diff: a changed fixture means the pipeline changed.
set -euo pipefail
ROOT="$(pwd)"
cargo build --quiet --locked
BIN="$ROOT/target/debug/npu-commit"
OUT="$ROOT/tests/fixtures/traces"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=Test GIT_AUTHOR_EMAIL=test@example.com GIT_AUTHOR_DATE=2026-01-01T00:00:00Z
export GIT_COMMITTER_NAME=Test GIT_COMMITTER_EMAIL=test@example.com GIT_COMMITTER_DATE=2026-01-01T00:00:00Z
mkdir -p "$OUT"
fresh() { rm -rf "$WORK/$1"; git init -q --initial-branch=main "$WORK/$1"; cd "$WORK/$1"; }
commit() { git commit -q --allow-empty -m "$1"; }

fresh conventional
for s in "feat(git): coleta o histórico de commits" "fix(diff): corrige o parse de renomeações" "docs: atualiza a documentação da fase 2"; do commit "$s"; done
git checkout -q -b feature/NPU-12-retry
printf 'pub fn retry() -> usize {\n    2\n}\n' > retry.rs; git add retry.rs
"$BIN" --backend mock \
  --mock-response $'Aqui está a mensagem:\n\nAdiciona a nova tentativa' \
  --mock-response $'```\nfeat(generate): adiciona a nova tentativa\n```' \
  --trace "$OUT/conventional-pt-retry.json" >/dev/null

fresh gitmoji
for s in "✨ add login page" "🐛 fix crash on logout" ":memo: update the readme"; do commit "$s"; done
printf '# Login\n\nHow to sign in.\n' > LOGIN.md; printf 'x\n' > Cargo.lock; git add .
"$BIN" --backend mock --mock-response "" --mock-response "<think>still thinking" \
  --trace "$OUT/gitmoji-fallback.json" >/dev/null 2>&1

fresh free
for s in "Add support for renames" "Fix crash when the diff is empty"; do commit "$s"; done
printf 'fn main() {}\n' > main.rs; git add main.rs
"$BIN" --backend mock \
  --mock-response $'<think>The diff adds main.</think>\nSure! Here is a commit message:\n"Add the main entry point."\n\nThis commit message describes the change.' \
  --trace "$OUT/free-en-chatty.json" >/dev/null

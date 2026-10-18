#!/usr/bin/env bash
# scripts/release-notes.sh on a throwaway repository: only commits since the
# previous v* tag, grouped by Conventional Commits type.
#
#   tests/scripts/test-release-notes.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@t

git init -q --initial-branch=main "$work/repo"
cd "$work/repo"
commit() { git commit -q --allow-empty -m "$1"; }
commit "feat: before the tag"
git tag v0.1.0
commit "feat(cli): adiciona o comando bench"
commit "fix: corrige o parse"
commit "docs: atualiza o guia"
commit "build: fixa o toolchain"
commit "Mensagem livre"

notes=$("$ROOT/scripts/release-notes.sh" v0.2.0)
failures=0
expect() {
    if grep -qF -- "$1" <<<"$notes"; then echo "ok    $1"; else
        echo "FAIL  expected: $1"
        failures=$((failures + 1))
    fi
}
refute() {
    if grep -qF -- "$1" <<<"$notes"; then
        echo "FAIL  unexpected: $1"
        failures=$((failures + 1))
    else echo "ok    no $1"; fi
}
expect "## smart-commit 0.2.0"
expect "Changes since v0.1.0."
expect "### Features"
expect "- **cli** adiciona o comando bench"
expect "### Fixes"
expect "- corrige o parse"
expect "### Documentation"
refute "- **** "
expect "### Other"
expect "- fixa o toolchain"
expect "- Mensagem livre"
refute "before the tag"
expect "install.sh | bash"

first=$("$ROOT/scripts/release-notes.sh" v0.1.0 v0.1.0)
if grep -q "First release." <<<"$first"; then
    echo "ok    first release"
else
    echo "FAIL  first release"
    failures=$((failures + 1))
fi
[ "$failures" = 0 ]

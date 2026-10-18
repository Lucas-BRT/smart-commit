#!/usr/bin/env bash
# Prints the release notes of TAG: the commits since the previous v* tag,
# grouped by their Conventional Commits type, plus how to install.
#
#   scripts/release-notes.sh v0.2.0 [REV]   (REV defaults to HEAD)
set -euo pipefail

TAG=${1:?usage: release-notes.sh TAG [REV]}
REV=${2:-HEAD}
previous=$(git describe --tags --match 'v*' --abbrev=0 "$REV^" 2>/dev/null || true)
range=${previous:+$previous..}$REV

subjects=$(git log --no-merges --format=%s "$range")

section() {
    local title=$1 pattern=$2 lines
    lines=$(grep -E "^($pattern)(\([^)]*\))?!?: " <<<"$subjects" || true)
    if [ -n "$lines" ]; then
        printf '### %s\n\n' "$title"
        sed -E 's/^[a-z]+(\(([^)]*)\))?!?: /- **\2** /; s/^- \*\*\*\* /- /' <<<"$lines"
        printf '\n'
    fi
}

echo "## smart-commit ${TAG#v}"
echo
if [ -n "$previous" ]; then
    echo "Changes since $previous."
else
    echo "First release."
fi
echo
section "Features" feat
section "Fixes" fix
section "Performance" perf
section "Documentation" docs
others=$(grep -vE "^(feat|fix|perf|docs)(\([^)]*\))?!?: " <<<"$subjects" || true)
if [ -n "$others" ]; then
    printf '### Other\n\n'
    sed -E 's/^[a-z]+(\(([^)]*)\))?!?: /- **\2** /; t; s/^/- /' <<<"$others" | sed 's/^- \*\*\*\* /- /'
    printf '\n'
fi
cat <<'EOF'
### Install

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash
```

The tarball is reproducible: `scripts/package.sh` on this tag gives the bytes
in `SHA256SUMS`, on any machine with Docker.
EOF

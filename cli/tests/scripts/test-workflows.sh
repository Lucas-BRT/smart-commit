#!/usr/bin/env bash
# Every workflow is valid YAML and every `uses:` is pinned by a commit SHA
# (40 hex digits). The YAML check needs python3 with PyYAML and is skipped
# without it. No network.
#
#   cli/tests/scripts/test-workflows.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
failures=0

for file in "$ROOT"/.github/workflows/*.yml; do
    name=${file#"$ROOT"/}
    unpinned=$(grep -E '^\s*(-\s+)?uses:' "$file" | grep -vE 'uses:\s*[^@ ]+@[0-9a-f]{40}(\s|$)' || true)
    if [ -z "$unpinned" ]; then
        echo "ok    $name pins every action by SHA"
    else
        echo "FAIL  $name has actions not pinned by a 40-hex SHA:"
        printf "        %s\n" "${unpinned//$'\n'/$'\n        '}"
        failures=$((failures + 1))
    fi
    if python3 -c 'import yaml' 2>/dev/null; then
        if python3 -c 'import sys, yaml; yaml.safe_load(open(sys.argv[1]))' "$file"; then
            echo "ok    $name is valid YAML"
        else
            echo "FAIL  $name is not valid YAML"
            failures=$((failures + 1))
        fi
    else
        echo "skip  $name YAML check (no PyYAML)"
    fi
done

# The release attests what it publishes.
if grep -q 'actions/attest-build-provenance@' "$ROOT/.github/workflows/release.yml" &&
    grep -q 'attestations: write' "$ROOT/.github/workflows/release.yml" &&
    grep -q 'id-token: write' "$ROOT/.github/workflows/release.yml"; then
    echo "ok    release.yml attests the assets"
else
    echo "FAIL  release.yml does not attest the assets"
    failures=$((failures + 1))
fi

[ "$failures" = 0 ] || {
    echo "$failures check(s) failed" >&2
    exit 1
}
echo "all checks passed"

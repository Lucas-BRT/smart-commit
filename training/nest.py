"""Nested subsets of an examples file for a learning curve: each size holds
every example of the smaller ones, ranked by the SHA-256 of the commit hash
(so the conventional/free mix stays about the same at every size).

    python -I nest.py EXAMPLES.jsonl OUT_PREFIX 2500 5000 20000 ...

writes OUT_PREFIX-2500.jsonl, OUT_PREFIX-5000.jsonl, ...
"""

import hashlib
import json
import sys
from pathlib import Path


def main() -> None:
    src, prefix, sizes = Path(sys.argv[1]), sys.argv[2], [int(n) for n in sys.argv[3:]]
    lines = src.read_text().splitlines()
    ranked = sorted(
        lines, key=lambda line: hashlib.sha256(json.loads(line)["hash"].encode()).hexdigest()
    )
    for n in sizes:
        assert n <= len(ranked), (n, len(ranked))
        subset = ranked[:n]
        conventional = sum(json.loads(line)["set"] == "conventional" for line in subset)
        Path(f"{prefix}-{n}.jsonl").write_text("".join(line + "\n" for line in subset))
        print(f"{prefix}-{n}.jsonl: {n} examples, {conventional} conventional")


if __name__ == "__main__":
    main()

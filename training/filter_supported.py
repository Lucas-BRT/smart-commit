"""Drops, from a selection already made, the commits a cleaner label set
leaves out: subjects `build_inputs.keep_target` now rejects (versions alone,
release bumps) and subjects naming a version, number or identifier found
neither in the diff nor in the history (`score.unsupported`). Unlike
`select.py --supported-only`, nothing else is chosen in their place, so the
result differs from its source only by the filter: how h3a was made from h1.

    python -I filter_supported.py SRC_DIR OUT_DIR
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from build_inputs import keep_target  # noqa: E402
from score import evidence, unsupported  # noqa: E402


def main() -> None:
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    inputs = (src / "inputs.jsonl").read_text().splitlines()
    meta = (src / "meta.jsonl").read_text().splitlines()
    assert len(inputs) == len(meta)
    kept = [
        (i, m)
        for i, m in zip(inputs, meta)
        if keep_target(json.loads(m)["target"])
        and not unsupported(json.loads(m)["target"], evidence(json.loads(i)))
    ]
    out.mkdir(parents=True, exist_ok=True)
    (out / "inputs.jsonl").write_text("".join(i + "\n" for i, _ in kept))
    (out / "meta.jsonl").write_text("".join(m + "\n" for _, m in kept))
    conventional = sum(json.loads(m)["set"] == "conventional" for _, m in kept)
    print(f"{len(inputs)} -> {len(kept)} commits, {conventional} conventional", file=sys.stderr)


if __name__ == "__main__":
    main()

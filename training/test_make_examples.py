"""Checks `make_examples`, the one training tool left in Python besides the
training itself; the data tools are in Rust (`sc-training`).

    python -I test_make_examples.py
"""

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from make_examples import make_examples  # noqa: E402


def test_make_examples_passes_the_rules() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        # A stand-in for smart-commit that answers with its own arguments.
        fake = d / "smart-commit"
        fake.write_text('#!/bin/sh\necho "{\\"system\\": \\"$*\\", \\"user\\": \\"u\\"}"\n')
        fake.chmod(0o755)
        (d / "inputs.jsonl").write_text("{}\n")
        meta = {"target": "feat: add x.", "repo": "r", "hash": "h", "set": "conventional"}
        (d / "meta.jsonl").write_text(json.dumps(meta) + "\n")
        for rules in ("full", "omitted"):
            make_examples(d, str(fake), rules)
            example = json.loads((d / "examples.jsonl").read_text())
            assert example["system"] == f"prompt --inputs {d / 'inputs.jsonl'} --rules {rules}"
            assert example["target"] == "feat: add x" and example["user"] == "u"


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

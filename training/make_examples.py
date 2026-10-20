"""Selected inputs -> training examples, with prompts built by smart-commit.

Runs `smart-commit prompt --inputs` with an empty XDG_CONFIG_HOME, so only
smart-commit's defaults shape the prompts, and writes `examples.jsonl`:
`{"system", "user", "target", "repo", "hash", "set"}` per line. `--rules
omitted` builds the prompts without the fixed instructions, for a model
trained to do without them (H6).

    python -I make_examples.py DIR SMART_COMMIT_BINARY [--rules full|omitted]
"""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


def make_examples(d: Path, binary: str, rules: str = "full") -> None:
    with tempfile.TemporaryDirectory() as config_home:
        env = {"XDG_CONFIG_HOME": config_home, "PATH": "/usr/bin:/bin"}
        prompts = subprocess.run(
            [binary, "prompt", "--inputs", str(d / "inputs.jsonl"), "--rules", rules],
            env=env, check=True, capture_output=True, text=True,
        ).stdout.splitlines()
    meta = [json.loads(line) for line in (d / "meta.jsonl").read_text().splitlines()]
    assert len(prompts) == len(meta)
    with open(d / "examples.jsonl", "w") as out:
        for p, m in zip(prompts, meta):
            prompt = json.loads(p)
            # The sanitizer drops a trailing period; train on what it keeps.
            target = m["target"].rstrip(".").rstrip()
            example = {**prompt, "target": target, "repo": m["repo"], "hash": m["hash"],
                       "set": m["set"]}
            out.write(json.dumps(example, ensure_ascii=False) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("dir", type=Path)
    parser.add_argument("binary")
    parser.add_argument("--rules", choices=["full", "omitted"], default="full")
    args = parser.parse_args()
    make_examples(args.dir, args.binary, args.rules)


if __name__ == "__main__":
    main()

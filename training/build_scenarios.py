"""Builds the commit scenarios of `scenarios/` (see scenarios.py).

    python -I build_scenarios.py build DIR SMART_COMMIT [--split train|eval|all] [--only PREFIX]
    python -I build_scenarios.py show DIR [--repo NAME | --case ID]
    python -I build_scenarios.py list

`build` writes DIR/inputs.jsonl and DIR/meta.jsonl, the layout of
`select.py`'s output: `make_examples.py DIR SMART_COMMIT` makes training
examples of them, `smart-commit eval DIR/inputs.jsonl` and `score.py DIR`
evaluate a model on them. A case whose outcome differs from the expected
one (smart-commit refuses to generate, or generates when it should
refuse) fails the build, unless the case documents it as a known `issue`.

`show` leaves a base repository, or a case applied on it, in DIR to look
at while writing cases.
"""

import argparse
import collections
import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from scenarios import ScenarioError, apply, build_repo, capture, load_cases, load_repos  # noqa: E402
from scenarios import meta_of, select  # noqa: E402


def build(out: Path, binary: str, split: str, only: str) -> int:
    repos = load_repos()
    cases = select(load_cases(repos=repos), split, only)
    out.mkdir(parents=True, exist_ok=True)
    failures = 0
    with open(out / "inputs.jsonl", "w") as inputs_file, open(out / "meta.jsonl", "w") as meta_file:
        for case in cases:
            with tempfile.TemporaryDirectory() as tmp:
                workdir = Path(tmp) / "repo"
                apply(case, repos[case.repo], workdir)
                inputs, error = capture(workdir, binary)
            refused = inputs is None
            if refused != (case.outcome == "error"):
                what = f"refused: {error}" if refused else "generated a message"
                if case.issue:
                    print(f"known issue  {case.id}: {what} ({case.issue})", file=sys.stderr)
                else:
                    print(f"FAILED       {case.id}: {what}", file=sys.stderr)
                    failures += 1
            if refused:
                continue
            inputs_file.write(json.dumps(inputs, ensure_ascii=False) + "\n")
            meta_file.write(json.dumps(meta_of(case, repos[case.repo], inputs), ensure_ascii=False) + "\n")
    print(f"{len(cases)} cases, {failures} failed", file=sys.stderr)
    return 1 if failures else 0


def show(out: Path, repo: str | None, case_id: str | None) -> int:
    repos = load_repos()
    if case_id:
        case = next((c for c in load_cases(repos=repos) if c.id == case_id), None)
        if case is None:
            print(f"no case {case_id}", file=sys.stderr)
            return 1
        apply(case, repos[case.repo], out)
    else:
        build_repo(repos[repo], out)
    print(out)
    return 0


def list_cases() -> int:
    cases = load_cases()
    table = collections.Counter((c.level, c.split) for c in cases)
    print("level  train  eval")
    for level in sorted({c.level for c in cases}):
        print(f"{level:>5}  {table[level, 'train']:>5}  {table[level, 'eval']:>4}")
    print(f"total  {sum(c.split == 'train' for c in cases):>5}  {sum(c.split == 'eval' for c in cases):>4}")
    for case in cases:
        flags = " ".join(filter(None, [case.outcome if case.outcome != "message" else "",
                                       "issue" if case.issue else ""]))
        print(f"  L{case.level} {case.split:<5} {case.id:<34} {case.title} {flags}".rstrip())
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    b = sub.add_parser("build")
    b.add_argument("out", type=Path)
    b.add_argument("binary")
    b.add_argument("--split", choices=["train", "eval", "all"], default="all")
    b.add_argument("--only", default="", help="only the cases whose id starts with this")
    s = sub.add_parser("show")
    s.add_argument("out", type=Path)
    group = s.add_mutually_exclusive_group(required=True)
    group.add_argument("--repo")
    group.add_argument("--case")
    sub.add_parser("list")
    args = parser.parse_args()
    try:
        if args.command == "build":
            return build(args.out, args.binary, args.split, args.only)
        if args.command == "show":
            return show(args.out, args.repo, args.case)
        return list_cases()
    except ScenarioError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

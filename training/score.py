"""Scores `smart-commit eval` traces against the real subjects.

    python -I score.py EVAL_DIR TRACES.jsonl [--json]

Per set (conventional, free) and overall:
- first_try: the first reply passed the sanitizer
- fallback: every attempt failed; the message came from the file list
- copied: the subject is one of the history subjects verbatim
- type_match: same Conventional type as the real subject (conventional set)
- scope_known: no scope, or one already used in the history (conventional set)
- word_f1: word overlap with the real subject, a cheap proxy for content
- exact: identical to the real subject

Metrics of the conventional set are averaged over that set in "all" too.
"""

import argparse
import collections
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from build_inputs import CONVENTIONAL  # noqa: E402

SCOPE = re.compile(r"^[a-z]+\(([^()]+)\)")
WORD = re.compile(r"[a-z0-9]+")


def words(text: str) -> list[str]:
    return WORD.findall(text.lower())


def f1(a: str, b: str) -> float:
    x, y = collections.Counter(words(a)), collections.Counter(words(b))
    common = sum((x & y).values())
    if not common:
        return 0.0
    p, r = common / sum(x.values()), common / sum(y.values())
    return 2 * p * r / (p + r)


def conv_type(subject: str) -> str | None:
    m = CONVENTIONAL.match(subject)
    return m.group("type") if m else None


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("eval_dir", type=Path)
    parser.add_argument("traces", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    meta = [json.loads(l) for l in (args.eval_dir / "meta.jsonl").read_text().splitlines()]
    traces = [json.loads(l) for l in args.traces.read_text().splitlines()]
    assert len(meta) == len(traces), (len(meta), len(traces))

    sums: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    counts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    types: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    for m, t in zip(meta, traces):
        g = t["generation"]
        subject = g["message"]["subject"]
        history = t["inputs"]["subjects"]
        row = {
            "first_try": g["attempts"][0]["rejected"] is None,
            "fallback": g["source"] == "fallback",
            "copied": subject in history,
            "word_f1": f1(subject, m["target"]),
            "exact": subject == m["target"],
        }
        if m["set"] == "conventional":
            row["type_match"] = conv_type(subject) == m["type"]
            scope = SCOPE.match(subject)
            known = {s.group(1) for h in history if (s := SCOPE.match(h))}
            row["scope_known"] = scope is None or scope.group(1) in known
        for key in (m["set"], "all"):
            sums[key]["n"] += 1
            for k, v in row.items():
                sums[key][k] += float(v)
                counts[key][k] += 1
            if m["set"] == "conventional":
                types[key][f"{m['type']}->{conv_type(subject)}"] += 1

    report = {}
    for key, c in sums.items():
        n = c.pop("n")
        report[key] = {"n": int(n), **{k: round(v / counts[key][k], 3) for k, v in sorted(c.items())}}
        if types[key]:
            report[key]["types (real->generated)"] = dict(types[key].most_common())
    if args.json:
        print(json.dumps(report, indent=2))
        return
    for key, r in report.items():
        print(f"[{key}] n={r.pop('n')}")
        for k, v in r.items():
            print(f"  {k:<14} {v}")


if __name__ == "__main__":
    main()

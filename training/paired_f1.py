"""The paired difference in `word_f1` between two runs of the same cases, with a bootstrap interval (H14).

`sc-training score --against` pairs the yes/no metrics (McNemar), but `word_f1` is a number per case: this takes
the difference of the two runs case by case, for all the cases and for each set (Conventional, free), and
resamples the cases BOOTSTRAPS times (seeded) for the 95% interval of the mean difference.

    python -I paired_f1.py SET_DIR ARM.jsonl CONTROL.jsonl

`ARM.jsonl` and `CONTROL.jsonl` are `smart-commit eval` traces, one per line, in the order of `SET_DIR/meta.jsonl`.
The words are the lowercase runs of letters and digits, as `sc-training score` counts them. Standard library only.
"""

import collections
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import bootstrap, bounds  # noqa: E402

BOOTSTRAPS = 2000
SEED = 7


def words(text: str) -> list[str]:
    return re.findall(r"[a-z0-9]+", text.lower())


def f1(a: str, b: str) -> float:
    """The F1 of the words two subjects share (each word counted as often as both have it)."""
    x, y = collections.Counter(words(a)), collections.Counter(words(b))
    common = sum((x & y).values())
    if not common:
        return 0.0
    precision, recall = common / sum(x.values()), common / sum(y.values())
    return 2 * precision * recall / (precision + recall)


def interval(diffs: list[float]) -> tuple[float, float, float]:
    """The mean and the 95% bootstrap interval of the mean of `diffs`."""
    n = len(diffs)
    if n == 0:
        return 0.0, 0.0, 0.0
    lo, hi = bounds(bootstrap(diffs, lambda sample: sum(sample) / n, SEED, BOOTSTRAPS))
    return sum(diffs) / n, lo, hi


def compare(targets: list[str], sets: list[str], arm: list[str], control: list[str]) -> dict:
    """The mean difference (arm minus control) and its interval, for all the cases and for each set."""
    diffs = [f1(a, t) - f1(c, t) for a, c, t in zip(arm, control, targets)]
    out = {"all": interval(diffs)}
    for name in sorted(set(sets)):
        out[name] = interval([d for d, s in zip(diffs, sets) if s == name])
    return out


def subjects(path: Path) -> list[str]:
    return [json.loads(line)["generation"]["message"]["subject"] for line in open(path)]


def main() -> None:
    set_dir, arm, control = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    meta = [json.loads(line) for line in open(set_dir / "meta.jsonl")]
    arm_subjects, control_subjects = subjects(arm), subjects(control)
    if not len(arm_subjects) == len(control_subjects) == len(meta):
        sys.exit(f"{len(arm_subjects)} and {len(control_subjects)} traces for {len(meta)} cases")
    result = compare([m["target"] for m in meta], [m["set"] for m in meta], arm_subjects, control_subjects)
    for name, (mean, low, high) in result.items():
        print(f"word_f1 {name:12s}: {mean:+.4f}  95% [{low:+.4f}, {high:+.4f}]")


if __name__ == "__main__":
    main()

"""Scores the commit type of the models against more than the repository's own label (H13).

`type_match` takes the real commit's type as the only right answer, but the same change can be a `fix` or a
`feat`, a `chore` or a `build`. This reads the traces `sc-training compare` leaves for a set, and the Conventional
commits among its meta lines, and reports, for each model:

- `strict`: the type equals the label (the `type_match` of the scores);
- `group`: the type is in the label's group, written down before the numbers: {chore, build, ci},
  {refactor, perf, style}, {test, tests};
- `behaviour`: as `group`, with `feat` and `fix` counted as one;
- `plausible`: the type is the label's, or a plain classifier (`type_probe.py`'s probabilities) gives it at
  least PLAUSIBLE of the probability: the signals in the commit allow it;
- `seen`: the type is one the repository's history uses, or the label's;
- `unseen_label`: how often the label is a type the history does not use (the model cannot know it);
- the strict match inside the commits whose label the history uses and the ones whose label it does not use;
- `shift`: half the sum of the differences between the predicted and the real shares of each type (0 is the same
  mix, 1 is disjoint);
- `all_agree_wrong`: the share of commits where every model gives one type and the label is another, a floor for
  how often the label is the odd one out;
- `any_right`: at least one model has the label;
- `control`: `plausible` and `group` of a type drawn at random from the real types' mix (the mean of
  CONTROL_DRAWS draws), what a reply that ignores the commit gets on the same measures.

Every share has a bootstrap 95% interval (BOOTSTRAPS resamples of the commits, seeded), and the pairs of models
have the commits one gets right and the other not.

    python -I type_review.py SET_DIR RUN_DIR/SET NAME... [--proba FILE] [--sheet FILE]
    python -I type_review.py --ratings FILE

`RUN_DIR/SET` holds `NAME.jsonl`, one trace per line, in the order of `SET_DIR/meta.jsonl`; the first NAME is the
model whose mistakes go on the sheet. `--proba` is the JSON of `type_probe.py` (classes and p for the Conventional
commits). `--sheet` writes the commits to rate by hand: `rating` is `A` (a maintainer reading the diff could
reasonably have used the model's type), `N` (the type misdescribes the change) or `?` (the excerpt does not say).
`--ratings` counts a rated sheet.
"""

import collections
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import bootstrap, bounds, type_of  # noqa: E402

GROUPS = [{"chore", "build", "ci"}, {"refactor", "perf", "style"}, {"test", "tests"}]
PLAUSIBLE = 0.15
BOOTSTRAPS = 2000
CONTROL_DRAWS = 200
SHEET_FILES = 4
SHEET_HUNK = 350
SEED = 7


def group_of(kind: str | None, behaviour: bool = False) -> str | None:
    """The name of the group a type is in (itself when it is in none)."""
    if kind is None:
        return None
    if behaviour and kind in ("feat", "fix"):
        return "feat|fix"
    for group in GROUPS:
        if kind in group:
            return "|".join(sorted(group))
    return kind


def interval(flags: list[bool]) -> tuple[float, float, float]:
    """The share of True and its 95% bootstrap interval."""
    n = len(flags)
    if n == 0:
        return 0.0, 0.0, 0.0
    lo, hi = bounds(bootstrap(flags, lambda sample: sum(sample) / n, SEED, BOOTSTRAPS))
    return sum(flags) / n, lo, hi


def shift(predicted: list[str | None], real: list[str]) -> float:
    """Half the L1 distance between the shares of the predicted and the real types."""
    n = len(real)
    p, r = collections.Counter(predicted), collections.Counter(real)
    return 0.5 * sum(abs(p[k] / n - r[k] / n) for k in set(p) | set(r))


def commits(set_dir: Path) -> list[dict]:
    """The Conventional commits of a set, with the index of their line (the trace's line)."""
    out = []
    for index, line in enumerate(open(set_dir / "meta.jsonl")):
        meta = json.loads(line)
        if meta.get("set") == "conventional" and meta.get("type"):
            out.append({
                "line": index, "type": meta["type"].lower(), "subject": meta["target"], "repo": meta["repo"],
                "hash": meta["hash"],
            })
    return out


def predictions(path: Path, lines: list[int]) -> list[dict]:
    """The traces of one model at those lines: its subject and its type (None for no type)."""
    wanted = set(lines)
    got = {}
    for index, line in enumerate(open(path)):
        if index in wanted:
            trace = json.loads(line)
            subject = trace["generation"]["message"]["subject"]
            got[index] = {"subject": subject, "type": type_of(subject), "trace": trace}
    return [got[i] for i in lines]


def history_types(trace: dict) -> set[str]:
    return {t for t in map(type_of, trace["inputs"].get("subjects") or []) if t}


def report(rows: list[dict], names: list[str], proba: dict | None) -> dict:
    """Every metric, for the commits in `rows` ({type, models: {name: {type}}, history})."""
    real = [r["type"] for r in rows]
    out = {"n": len(rows), "models": {}, "pairs": {}}
    seen_label = [r["type"] in r["history"] for r in rows]
    out["unseen_label"] = interval([not s for s in seen_label])
    allowed = None
    if proba:
        classes = proba["classes"]
        allowed = [{c for c, p in zip(classes, row) if p >= PLAUSIBLE} for row in proba["p"]]
    if allowed:
        rng = random.Random(SEED)
        plausible = group = 0.0
        for _ in range(CONTROL_DRAWS):
            drawn = [rng.choice(real) for _ in rows]
            plausible += sum(d == t or d in a for d, t, a in zip(drawn, real, allowed)) / len(rows)
            group += sum(group_of(d) == group_of(t) for d, t in zip(drawn, real)) / len(rows)
        out["control"] = {"plausible": plausible / CONTROL_DRAWS, "group": group / CONTROL_DRAWS}
    for name in names:
        got = [r["models"][name]["type"] for r in rows]
        strict = [g == t for g, t in zip(got, real)]
        entry = {
            "strict": interval(strict),
            "group": interval([group_of(g) == group_of(t) and g is not None for g, t in zip(got, real)]),
            "behaviour": interval(
                [group_of(g, True) == group_of(t, True) and g is not None for g, t in zip(got, real)]
            ),
            "seen": interval([g is not None and (g in r["history"] or g == r["type"]) for g, r in zip(got, rows)]),
            "shift": shift(got, real),
            "strict_label_seen": interval([s for s, k in zip(strict, seen_label) if k]),
            "strict_label_unseen": interval([s for s, k in zip(strict, seen_label) if not k]),
        }
        if allowed:
            entry["plausible"] = interval([s or (g in a) for s, g, a in zip(strict, got, allowed)])
        out["models"][name] = entry
    agree = [
        len({r["models"][n]["type"] for n in names}) == 1 and r["models"][names[0]]["type"] != r["type"]
        for r in rows
    ]
    out["all_agree_wrong"] = interval(agree)
    out["any_right"] = interval([any(r["models"][n]["type"] == r["type"] for n in names) for r in rows])
    for i, a in enumerate(names):
        for b in names[i + 1 :]:
            only_a = sum(r["models"][a]["type"] == r["type"] != r["models"][b]["type"] for r in rows)
            only_b = sum(r["models"][b]["type"] == r["type"] != r["models"][a]["type"] for r in rows)
            out["pairs"][f"{a} vs {b}"] = (only_a, only_b)
    return out


def sheet(rows: list[dict], name: str, per_pair: int = 4, limit: int = 60) -> list[dict]:
    """The commits a model gets 'wrong', a few for each (label -> predicted) pair, for rating by hand."""
    by_pair = collections.defaultdict(list)
    for r in rows:
        got = r["models"][name]["type"]
        if got != r["type"]:
            by_pair[(r["type"], got)].append(r)
    picked = []
    rng = random.Random(SEED)
    for pair in sorted(by_pair, key=lambda p: -len(by_pair[p])):
        bucket = by_pair[pair]
        rng.shuffle(bucket)
        picked.extend(bucket[:per_pair])
    rng.shuffle(picked)
    return picked[:limit]


def rated(sheet_rows: list[dict]) -> dict:
    """How many commits got each rating, and the share of A among the ones that could be told (A or N)."""
    counts = collections.Counter(r.get("rating") for r in sheet_rows)
    told = counts["A"] + counts["N"]
    return {"A": counts["A"], "N": counts["N"], "?": counts["?"], "unrated": counts[None],
            "acceptable": counts["A"] / told if told else 0.0}


def main() -> None:
    args = sys.argv[1:]
    if args[:1] == ["--ratings"]:
        print(json.dumps(rated(json.loads(Path(args[1]).read_text()))))
        return
    proba_path = sheet_path = None
    for flag in ("--proba", "--sheet"):
        if flag in args:
            at = args.index(flag)
            value = Path(args[at + 1])
            del args[at : at + 2]
            proba_path, sheet_path = (value, sheet_path) if flag == "--proba" else (proba_path, value)
    set_dir, run_dir, names = Path(args[0]), Path(args[1]), args[2:]
    base = commits(set_dir)
    lines = [c["line"] for c in base]
    per_model = {n: predictions(run_dir / f"{n}.jsonl", lines) for n in names}
    rows = []
    for i, c in enumerate(base):
        models = {n: per_model[n][i] for n in names}
        rows.append({**c, "models": models, "history": history_types(models[names[0]]["trace"])})
    proba = json.loads(proba_path.read_text()) if proba_path else None
    print(json.dumps(report(rows, names, proba), indent=1))
    if sheet_path:
        picked = sheet(rows, names[0])
        sheet_path.write_text(json.dumps([
            {
                "repo": r["repo"], "hash": r["hash"], "real": r["subject"], "model": r["models"][names[0]]["subject"],
                "files": [
                    {"path": f["path"], "lines": f["lines"], "hunks": (f.get("hunks") or "")[:SHEET_HUNK]}
                    for f in r["models"][names[0]]["trace"]["inputs"]["files"][:SHEET_FILES]
                ],
            }
            for r in picked
        ], indent=1))


if __name__ == "__main__":
    main()

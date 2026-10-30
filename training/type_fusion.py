"""Three ways to improve the type's distribution without training (H15), tried on what `type_confidence.py` collected.

Each starts from the model's probability for each type at the first token of the reply (`type_confidence.py
collect`: `cases` with `real`, `allowed` and `dist`, the likeliest first) and changes it, or the choice of model:

- `prior`: the distribution times the repository's own use of each type, `p(t) * ((count_t + 1) / (n + K)) ** lam`,
  with the counts of the types of the history's subjects and K the types in play (what the prompt lists as used);
- `fuse`: the distribution times a plain classifier's (`type_probe.py` probabilities), `p(t) * (clf(t) + EPS) ** w`,
  a product of experts rather than the classifier overriding the model;
- `cascade`: the small model answers when it is at least `tau` sure of its type and the big one otherwise;
  the share the big one answers is its cost.

The weight (`lam`, `w`, `tau`) is chosen on the calibration set and read on the test set, as the other results.

    python -I type_fusion.py prior CAL.json CAL_TRACES.jsonl TEST.json TEST_TRACES.jsonl
    python -I type_fusion.py fuse CAL.json CAL_CLF.json TEST.json TEST_CLF.json
    python -I type_fusion.py cascade SMALL_CAL.json SMALL_CAL_TRACES.jsonl BIG_CAL.json BIG_CAL_TRACES.jsonl \\
        SMALL_TEST.json SMALL_TEST_TRACES.jsonl BIG_TEST.json BIG_TEST_TRACES.jsonl SET_CAL SET_TEST

`*_TRACES.jsonl` are the `smart-commit eval` traces of the same set (the history is in their inputs); `*_CLF.json`
is the file `type_probe.py` writes beside the predictions (`.proba`: classes and one row of probabilities per
Conventional commit, in order). Standard library only.
"""

import collections
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import type_of as common_type_of  # noqa: E402
from type_confidence import score  # noqa: E402

EPS = 1e-3
LAMBDAS = [0.0, 0.25, 0.5, 0.75, 1.0, 1.5]
WEIGHTS = [0.0, 0.25, 0.5, 0.75, 1.0, 1.5]
TAUS = [0.0, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.01]


def type_of(subject: str | None) -> str | None:
    kind = common_type_of(subject)
    return "test" if kind == "tests" else kind


def normalise(dist: list, allowed: list[str]) -> list[list]:
    """The types the repository uses (all when none are listed), likeliest first, adding up to 1."""
    kept = [(k, p) for k, p in dist if not allowed or k in allowed]
    total = sum(p for _, p in kept)
    if total <= 0:
        return []
    return [[k, p / total] for k, p in sorted(kept, key=lambda kv: (-kv[1], kv[0]))]


def reweigh(dist: list, factors: dict[str, float]) -> list[list]:
    """The distribution times a factor per type, renormalised, likeliest first."""
    raw = [(k, p * factors.get(k, 1.0)) for k, p in dist]
    total = sum(p for _, p in raw)
    if total <= 0:
        return [list(x) for x in dist]
    return [[k, p / total] for k, p in sorted(raw, key=lambda kv: (-kv[1], kv[0]))]


def history_counts(traces: list[dict]) -> list[collections.Counter]:
    """The count of each type among the subjects of each trace's history."""
    return [
        collections.Counter(t for t in map(type_of, tr["inputs"].get("subjects") or []) if t) for tr in traces
    ]


def prior_factors(counts: collections.Counter, types: list[str], lam: float) -> dict[str, float]:
    """`((count + 1) / (n + K)) ** lam` for each type: how much the repository uses it, smoothed."""
    n, k = sum(counts[t] for t in types), len(types)
    return {t: ((counts[t] + 1) / (n + k)) ** lam for t in types}


def with_prior(cases: list[dict], counts: list[collections.Counter], lam: float) -> list[dict]:
    out = []
    for case, history in zip(cases, counts):
        base = normalise(case["dist"], case.get("allowed", []))
        factors = prior_factors(history, [k for k, _ in base], lam)
        out.append({**case, "dist": reweigh(base, factors)})
    return out


def classifier_rows(proba: dict) -> list[dict[str, float]]:
    """One {type: probability} per commit, `tests` folded into `test`."""
    rows = []
    for row in proba["p"]:
        merged: dict[str, float] = {}
        for kind, p in zip(proba["classes"], row):
            kind = "test" if kind == "tests" else kind
            merged[kind] = merged.get(kind, 0.0) + p
        rows.append(merged)
    return rows


def with_classifier(cases: list[dict], rows: list[dict[str, float]], w: float) -> list[dict]:
    out = []
    for case, clf in zip(cases, rows):
        base = normalise(case["dist"], case.get("allowed", []))
        factors = {k: (clf.get(k, 0.0) + EPS) ** w for k, _ in base}
        out.append({**case, "dist": reweigh(base, factors)})
    return out


def cascade(small: list[dict], big: list[dict], tau: float) -> tuple[list[str | None], float]:
    """The type each commit gets and the share the big model answered: the small one's when it is `tau` sure."""
    types, escalated = [], 0
    for s, b in zip(small, big):
        if s["dist"] and s["dist"][0][1] >= tau:
            types.append(s["dist"][0][0])
        else:
            escalated += 1
            types.append(b["dist"][0][0] if b["dist"] else None)
    return types, escalated / len(small)


def measures(cases: list[dict]) -> dict:
    """`type_match`, the negative log-likelihood and the calibration error of the distributions."""
    s = score(cases)
    return {"type_match": s["top1"], "nll": s["nll"], "ece": s["ece"], "auroc": s["auroc"]}


def best(options: dict[float, list[dict]]) -> float:
    """The weight with the best type_match on the calibration set; the lower negative log-likelihood and then the
    smaller weight break ties."""
    rows = {w: measures(cases) for w, cases in options.items()}
    return min(rows, key=lambda w: (-rows[w]["type_match"], rows[w]["nll"], w))


def load_cases(path: Path) -> list[dict]:
    return json.loads(path.read_text())["cases"]


def load_traces(path: Path, cases: list[dict]) -> list[dict]:
    """The traces at the lines of `cases` (the Conventional commits of the set)."""
    wanted = {c["line"] for c in cases}
    got = {i: json.loads(line) for i, line in enumerate(open(path)) if i in wanted}
    return [got[c["line"]] for c in cases]


def show(name: str, rows: dict) -> None:
    print(f"  {name:28s} type_match {rows['type_match']:.3f}  NLL {rows['nll']:.3f}  ECE {rows['ece']:.3f}  "
          f"AUROC {rows['auroc']:.3f}")


def run_weighted(kind: str, cal: list[dict], cal_aux, test: list[dict], test_aux, grid: list[float]) -> None:
    build = with_prior if kind == "prior" else with_classifier
    cal_options = {w: build(cal, cal_aux, w) for w in grid}
    chosen = best(cal_options)
    print(f"{kind}: weight chosen on the calibration set: {chosen}")
    show("calibration, as collected", measures(cal))
    show(f"calibration, weight {chosen}", measures(cal_options[chosen]))
    print("  (type_match by weight on the calibration set: " + ", ".join(
        f"{w}: {measures(c)['type_match']:.3f}" for w, c in cal_options.items()) + ")")
    show("test, as collected", measures(test))
    zero, shown = build(test, test_aux, 0.0), build(test, test_aux, chosen)
    show("test, weight 0 (allowed types)", measures(zero))
    show(f"test, weight {chosen}", measures(shown))
    before, after = right(zero), right(shown)
    gained = sum(y and not x for x, y in zip(before, after))
    lost = sum(x and not y for x, y in zip(before, after))
    print(f"  test, against weight 0: +{gained} / -{lost} commits of {len(test)}")


def right(cases: list[dict]) -> list[bool]:
    """Whether the likeliest type is the real one, for each commit."""
    return [bool(c["dist"]) and c["dist"][0][0] == c["real"] for c in cases]


def main() -> None:
    args = sys.argv[1:]
    if args[:1] == ["prior"]:
        cal, test = load_cases(Path(args[1])), load_cases(Path(args[3]))
        run_weighted("prior", cal, history_counts(load_traces(Path(args[2]), cal)), test,
                     history_counts(load_traces(Path(args[4]), test)), LAMBDAS)
    elif args[:1] == ["fuse"]:
        cal, test = load_cases(Path(args[1])), load_cases(Path(args[3]))
        run_weighted("fuse", cal, classifier_rows(json.loads(Path(args[2]).read_text())), test,
                     classifier_rows(json.loads(Path(args[4]).read_text())), WEIGHTS)
    elif args[:1] == ["cascade"]:
        sc, sct, bc, bct, st, stt, bt, btt, set_cal, set_test = args[1:11]
        report_cascade(Path(sc), Path(sct), Path(bc), Path(bct), Path(st), Path(stt), Path(bt), Path(btt),
                       Path(set_cal), Path(set_test))
    else:
        sys.exit(__doc__)


TYPE_MARGIN = 0.008
WORD_MARGIN = 0.010


def choose_tau(rows: dict[float, dict]) -> float:
    """The tau that makes the big model answer least while keeping the type_match and the word_f1 within the
    margins of the big model alone (the last tau, above 1); the highest tau among equals."""
    big = rows[max(rows)]
    ok = [t for t, r in rows.items()
          if r["type_match"] >= big["type_match"] - TYPE_MARGIN and r["word_f1"] >= big["word_f1"] - WORD_MARGIN]
    return min(ok, key=lambda t: (rows[t]["big"], -t))


def subjects_and_targets(traces: list[dict], cases: list[dict], set_dir: Path) -> tuple[list[str], list[str]]:
    meta = [json.loads(line) for line in open(set_dir / "meta.jsonl")]
    return [t["generation"]["message"]["subject"] for t in traces], [meta[c["line"]]["target"] for c in cases]


def report_cascade(sc, sct, bc, bct, st, stt, bt, btt, set_cal: Path, set_test: Path) -> None:
    from paired_f1 import f1

    small_cal, big_cal, small_test, big_test = load_cases(sc), load_cases(bc), load_cases(st), load_cases(bt)
    pieces = {}
    for name, small, big, small_traces, big_traces, set_dir in (
        ("calibration", small_cal, big_cal, sct, bct, set_cal),
        ("test", small_test, big_test, stt, btt, set_test),
    ):
        s_sub, targets = subjects_and_targets(load_traces(small_traces, small), small, set_dir)
        b_sub, _ = subjects_and_targets(load_traces(big_traces, big), big, set_dir)
        pieces[name] = (small, big, s_sub, b_sub, targets)

    def rows(name: str, tau: float) -> dict:
        small, big, s_sub, b_sub, targets = pieces[name]
        types, share = cascade(small, big, tau)
        sure = [bool(c["dist"]) and c["dist"][0][1] >= tau for c in small]
        subjects = [sm if ok else bg for ok, sm, bg in zip(sure, s_sub, b_sub)]
        return {
            "type_match": sum(t == c["real"] for t, c in zip(types, small)) / len(small),
            "word_f1": sum(f1(s, t) for s, t in zip(subjects, targets)) / len(small),
            "big": share,
        }

    assert [c["line"] for c in small_cal] == [c["line"] for c in big_cal]
    assert [c["line"] for c in small_test] == [c["line"] for c in big_test]
    cal_rows = {tau: rows("calibration", tau) for tau in TAUS}
    chosen = choose_tau(cal_rows)
    small_alone, big_alone = rows("test", 0.0), rows("test", 1.01)
    print("calibration set (the small model alone is tau 0, the big one alone tau > 1):")
    for tau, r in cal_rows.items():
        print(f"  tau {tau:4.2f}: big answers {r['big']:.3f}, type_match {r['type_match']:.3f}, word_f1 {r['word_f1']:.3f}")
    print("test set:")
    for tau in TAUS:
        r = rows("test", tau)
        print(f"  tau {tau:4.2f}: big answers {r['big']:.3f}, type_match {r['type_match']:.3f}, word_f1 {r['word_f1']:.3f}")
    print(f"tau chosen on the calibration set: {chosen}")
    r = rows("test", chosen)
    big = rows("test", 1.01)
    print(f"  test at tau {chosen}: big answers {r['big']:.3f}, type_match {r['type_match']:.3f} "
          f"({r['type_match'] - big['type_match']:+.3f} against the big model), word_f1 {r['word_f1']:.3f} "
          f"({r['word_f1'] - big['word_f1']:+.3f})")
    print(f"  small alone type_match {small_alone['type_match']:.3f}, big alone {big_alone['type_match']:.3f}; "
          f"word_f1 {small_alone['word_f1']:.3f} and {big_alone['word_f1']:.3f}")


if __name__ == "__main__":
    main()

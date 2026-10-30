"""Checks `type_fusion` on a few distributions made up for it.

    python -I test_type_fusion.py
"""

import collections
import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from type_fusion import (  # noqa: E402
    best, cascade, choose_tau, classifier_rows, history_counts, load_traces, normalise, prior_factors, reweigh, right, type_of,
    with_classifier, with_prior,
)


def case(real: str, *dist: tuple[str, float], allowed: list[str] | None = None, line: int = 0) -> dict:
    return {"real": real, "dist": [list(d) for d in dist], "allowed": allowed or [], "line": line}


def test_types_are_read_from_a_subject() -> None:
    assert type_of("Fix(core)!: x") == "fix" and type_of("tests: y") == "test" and type_of("update") is None


def test_the_distribution_keeps_the_types_the_repository_uses() -> None:
    got = normalise([["feat", 0.5], ["revert", 0.3], ["fix", 0.2]], ["fix", "feat"])
    assert [k for k, _ in got] == ["feat", "fix"] and abs(got[0][1] - 0.5 / 0.7) < 1e-9
    assert [k for k, _ in normalise([["fix", 0.6], ["feat", 0.4]], [])] == ["fix", "feat"]      # none listed: all
    assert normalise([["revert", 1.0]], ["fix"]) == []


def test_reweighing_renormalises_and_orders() -> None:
    got = reweigh([["feat", 0.5], ["fix", 0.5]], {"feat": 1.0, "fix": 3.0})
    assert [k for k, _ in got] == ["fix", "feat"]
    assert abs(got[0][1] - 0.75) < 1e-9 and abs(got[1][1] - 0.25) < 1e-9
    assert reweigh([["feat", 1.0]], {"feat": 0.0}) == [["feat", 1.0]]                          # nothing left: as it was


def test_the_prior_is_the_smoothed_use_of_each_type() -> None:
    counts = collections.Counter({"fix": 3, "feat": 1})
    f = prior_factors(counts, ["fix", "feat", "docs"], 1.0)
    assert abs(f["fix"] - 4 / 7) < 1e-9 and abs(f["feat"] - 2 / 7) < 1e-9 and abs(f["docs"] - 1 / 7) < 1e-9
    assert prior_factors(counts, ["fix", "feat"], 0.0) == {"fix": 1.0, "feat": 1.0}
    assert abs(prior_factors(counts, ["fix", "feat"], 0.5)["fix"] - (4 / 6) ** 0.5) < 1e-9


def test_a_repository_that_writes_fix_moves_an_even_split_to_fix() -> None:
    cases = [case("fix", ("feat", 0.5), ("fix", 0.5))]
    history = [collections.Counter({"fix": 3})]
    got = with_prior(cases, history, 1.0)[0]["dist"]
    # fix (3 + 1) / (3 + 2) = 0.8, feat 1 / 5 = 0.2: 0.5 * 0.8 against 0.5 * 0.2.
    assert got[0][0] == "fix" and abs(got[0][1] - 0.8) < 1e-9
    assert with_prior(cases, history, 0.0)[0]["dist"][0][0] == "feat"                     # weight 0: the tie goes by name


def test_history_counts_read_the_types_of_the_subjects() -> None:
    traces = [{"inputs": {"subjects": ["fix: a", "feat(x): b", "fix: c", "update d"]}}, {"inputs": {}}]
    got = history_counts(traces)
    assert got[0] == collections.Counter({"fix": 2, "feat": 1}) and got[1] == collections.Counter()


def test_the_classifier_multiplies_the_distribution() -> None:
    rows = classifier_rows({"classes": ["feat", "fix", "test", "tests"], "p": [[0.9, 0.1, 0.0, 0.0], [0.1, 0.1, 0.5, 0.3]]})
    assert rows[1]["test"] == 0.8 and "tests" not in rows[1]
    cases = [case("fix", ("feat", 0.5), ("fix", 0.5))]
    got = with_classifier(cases, rows[:1], 1.0)[0]["dist"]
    # feat 0.5 * (0.9 + 0.001), fix 0.5 * (0.1 + 0.001)
    assert got[0][0] == "feat" and abs(got[0][1] - 0.901 / 1.002) < 1e-9
    assert with_classifier(cases, rows[:1], 0.0)[0]["dist"][0][0] == "feat"               # weight 0: the tie goes by name
    # A type the classifier never gives keeps a small share, not zero.
    out = with_classifier([case("docs", ("fix", 0.6), ("docs", 0.4))], rows[:1], 1.0)[0]["dist"]
    assert {k for k, _ in out} == {"fix", "docs"}


def test_the_small_model_answers_when_it_is_sure() -> None:
    small = [case("fix", ("fix", 0.9), ("feat", 0.1)), case("feat", ("fix", 0.6), ("feat", 0.4))]
    big = [case("fix", ("feat", 0.8), ("fix", 0.2)), case("feat", ("feat", 0.7), ("fix", 0.3))]
    assert cascade(small, big, 0.0) == (["fix", "fix"], 0.0)
    assert cascade(small, big, 0.8) == (["fix", "feat"], 0.5)                             # the second is not sure enough
    assert cascade(small, big, 1.01) == (["feat", "feat"], 1.0)
    assert cascade([case("x")], [case("x")], 0.5) == ([None], 1.0)                       # no type from either


def test_the_best_weight_is_the_most_accurate_then_the_best_likelihood_then_the_smallest() -> None:
    sure_right = case("fix", ("fix", 0.9), ("feat", 0.1))
    unsure_right = case("fix", ("fix", 0.55), ("feat", 0.45))
    options = {0.0: [unsure_right], 0.5: [sure_right], 1.0: [sure_right]}
    assert best(options) == 0.5
    assert best({0.0: [unsure_right], 1.0: [case("fix", ("feat", 0.6), ("fix", 0.4))]}) == 0.0
    assert right([sure_right, case("fix", ("feat", 1.0)), case("fix")]) == [True, False, False]


def test_the_tau_is_the_one_that_asks_the_big_model_least_within_the_margins() -> None:
    rows = {
        0.0: {"type_match": 0.60, "word_f1": 0.30, "big": 0.0},
        0.5: {"type_match": 0.645, "word_f1": 0.345, "big": 0.3},
        0.7: {"type_match": 0.648, "word_f1": 0.349, "big": 0.5},
        1.01: {"type_match": 0.652, "word_f1": 0.350, "big": 1.0},
    }
    assert choose_tau(rows) == 0.5            # 0.645 is within 0.008 of 0.652 and 0.345 within 0.010 of 0.350
    rows[0.5]["word_f1"] = 0.33               # too far in the words: the next one
    assert choose_tau(rows) == 0.7
    only_big = {0.0: {"type_match": 0.5, "word_f1": 0.2, "big": 0.0}, 1.01: {"type_match": 0.7, "word_f1": 0.4, "big": 1.0}}
    assert choose_tau(only_big) == 1.01


def test_traces_are_read_at_the_lines_of_the_cases() -> None:
    with tempfile.TemporaryDirectory() as d:
        path = Path(d) / "t.jsonl"
        path.write_text("".join(json.dumps({"n": n}) + "\n" for n in range(5)))
        assert [t["n"] for t in load_traces(path, [case("a", line=3), case("b", line=1)])] == [3, 1]


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

"""Checks the measures of `type_review` on a few commits made up for it.

    python -I test_type_review.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from type_review import group_of, interval, rated, report, sheet, shift, type_of  # noqa: E402


def row(real: str, history: set[str], **models: str | None) -> dict:
    return {"type": real, "history": history, "repo": "r", "hash": "h", "subject": f"{real}: x",
            "models": {n: {"type": t, "subject": f"{t}: y"} for n, t in models.items()}}


def test_types_and_groups() -> None:
    assert type_of("Fix(core)!: x") == "fix" and type_of("fix/skill: x") is None and type_of(None) is None
    assert group_of("build") == group_of("chore") == group_of("ci")
    assert group_of("tests") == group_of("test") and group_of("docs") == "docs" and group_of(None) is None
    assert group_of("feat") != group_of("fix")
    assert group_of("feat", True) == group_of("fix", True) != group_of("docs", True)


def test_interval_and_shift() -> None:
    share, low, high = interval([True] * 60 + [False] * 40)
    assert share == 0.6 and low < 0.6 < high and 0.45 < low and high < 0.75
    assert interval([]) == (0.0, 0.0, 0.0)
    assert interval([True] * 10) == (1.0, 1.0, 1.0)
    assert shift(["fix", "fix"], ["fix", "feat"]) == 0.5
    assert shift(["a", "b"], ["a", "b"]) == 0.0 and shift(["a"], ["b"]) == 1.0


def test_report_counts_each_measure() -> None:
    rows = [
        row("fix", {"fix", "feat"}, a="fix", b="feat"),
        row("chore", {"chore"}, a="ci", b="ci"),
        row("feat", {"feat"}, a="fix", b=None),
        row("docs", {"fix"}, a="docs", b="docs"),
    ]
    proba = {"classes": ["chore", "ci", "docs", "feat", "fix"],
             "p": [[0, 0, 0, 0.5, 0.5], [0.5, 0.4, 0, 0, 0], [0, 0, 0, 0.5, 0.5], [0, 0, 1, 0, 0]]}
    out = report(rows, ["a", "b"], proba)
    a, b = out["models"]["a"], out["models"]["b"]
    assert out["n"] == 4 and a["strict"][0] == 0.5 and b["strict"][0] == 0.25
    assert a["group"][0] == 0.75 and b["group"][0] == 0.5          # ci is chore's group; a missing type is a miss
    assert a["behaviour"][0] == 1.0 and b["behaviour"][0] == 0.75  # fix for feat counts as one
    assert a["plausible"][0] == 1.0 and b["plausible"][0] == 0.75  # a miss (None) is not plausible
    assert a["seen"][0] == 0.5 and b["seen"][0] == 0.5             # `ci` is in no history; the label counts as seen
    assert out["unseen_label"][0] == 0.25                           # docs is not in its history
    assert out["any_right"][0] == 0.5 and out["all_agree_wrong"][0] == 0.25
    assert out["pairs"] == {"a vs b": (1, 0)}
    assert a["strict_label_unseen"][0] == 1.0 and a["strict_label_seen"][0] == 1 / 3
    assert 0 < out["control"]["plausible"] < 1 and 0 < out["control"]["group"] < 1


def test_the_sheet_takes_a_few_of_each_mistake() -> None:
    rows = [row("fix", set(), a="feat") for _ in range(9)] + [row("docs", set(), a="fix")] + [row("fix", set(), a="fix")]
    got = sheet(rows, "a", per_pair=2, limit=10)
    assert sorted((r["type"], r["models"]["a"]["type"]) for r in got) == [("docs", "fix"), ("fix", "feat"), ("fix", "feat")]
    assert len(sheet(rows, "a", per_pair=2, limit=2)) == 2
    assert sheet(rows[-1:], "a") == []


def test_ratings_count_what_could_be_told() -> None:
    got = rated([{"rating": "A"}, {"rating": "A"}, {"rating": "N"}, {"rating": "?"}, {}])
    assert got == {"A": 2, "N": 1, "?": 1, "unrated": 1, "acceptable": 2 / 3}
    assert rated([])["acceptable"] == 0.0


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

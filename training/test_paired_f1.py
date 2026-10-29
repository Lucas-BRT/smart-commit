"""Checks `paired_f1` on a few subjects made up for it.

    python -I test_paired_f1.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from paired_f1 import compare, f1, interval, words  # noqa: E402


def test_words_are_lowercase_runs_of_letters_and_digits() -> None:
    assert words("Fix(CLI): parse -C v1.2") == ["fix", "cli", "parse", "c", "v1", "2"]


def test_f1_counts_shared_words_as_often_as_both_have_them() -> None:
    assert f1("fix: parse flags", "fix: parse flags") == 1.0
    assert f1("a b", "c d") == 0.0
    assert f1("", "a") == 0.0
    # "fix parse" against "fix parse flags": 2 shared, precision 1, recall 2/3.
    assert abs(f1("fix parse", "fix parse flags") - 0.8) < 1e-9
    # a word twice in one and once in the other counts once.
    assert abs(f1("a a b", "a b") - 0.8) < 1e-9


def test_interval_has_the_mean_inside() -> None:
    mean, low, high = interval([0.1, 0.0, 0.2, -0.1, 0.05, 0.15])
    assert abs(mean - 0.4 / 6) < 1e-9 and low < mean < high
    assert interval([]) == (0.0, 0.0, 0.0)
    assert interval([0.5] * 10) == (0.5, 0.5, 0.5)


def test_compare_pairs_the_cases_and_splits_by_set() -> None:
    targets = ["fix: parse flags", "update the docs", "fix: parse flags", "update the docs"]
    sets = ["conventional", "free", "conventional", "free"]
    arm = ["fix: parse flags", "update the docs", "chore: x", "update"]
    control = ["chore: x", "x", "chore: x", "update the docs"]
    got = compare(targets, sets, arm, control)
    # conventional: (1 - 0) and (0 - 0) average 0.5; free: (1 - 0) and (0.5 - 1) average 0.25.
    assert abs(got["conventional"][0] - 0.5) < 1e-9
    assert abs(got["free"][0] - 0.25) < 1e-9
    assert set(got) == {"all", "conventional", "free"}
    assert abs(got["all"][0] - (got["conventional"][0] + got["free"][0]) / 2) < 1e-9


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

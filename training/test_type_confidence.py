"""Checks the measures of `type_confidence` on a few distributions made up for it (no server needed).

    python -I test_type_confidence.py
"""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from type_confidence import auroc, ece, from_logprobs, score, type_of  # noqa: E402


def test_tokens_are_read_like_the_product_reads_them() -> None:
    assert type_of(" Fix") == "fix" and type_of("tests") == "test" and type_of("ref") == "refactor"
    assert type_of("re") is None          # `refactor` and `revert`
    assert type_of("a") is None and type_of("hello") is None


def test_types_are_normalised_and_tokens_of_one_type_add_up() -> None:
    got = from_logprobs([("fix", math.log(0.5)), ("Fix", math.log(0.1)), ("feat", math.log(0.3)), ("The", math.log(0.1))])
    assert [k for k, _ in got] == ["fix", "feat"]
    assert abs(got[0][1] - 0.6 / 0.9) < 1e-9 and abs(sum(p for _, p in got) - 1) < 1e-9
    assert from_logprobs([("The", -1.0)]) == []
    tie = from_logprobs([("fix", math.log(0.5)), ("docs", math.log(0.5))])
    assert [k for k, _ in tie] == ["docs", "fix"]          # ties by name


def test_auroc() -> None:
    assert auroc([0.9, 0.8, 0.3, 0.2], [True, True, False, False]) == 1.0
    assert auroc([0.2, 0.3, 0.8, 0.9], [True, True, False, False]) == 0.0
    assert auroc([0.5, 0.5], [True, False]) == 0.5
    assert auroc([0.5, 0.6], [True, True]) == 0.5        # nothing to tell apart


def test_ece() -> None:
    assert ece([1.0] * 4, [True] * 4) == 0.0
    # 0.9 sure and right half of the time: the gap is 0.4
    assert abs(ece([0.9] * 4, [True, True, False, False]) - 0.4) < 1e-9
    assert abs(ece([0.95, 0.15], [True, False]) - (0.5 * 0.05 + 0.5 * 0.15)) < 1e-9


def case(real: str, *dist: tuple[str, float]) -> dict:
    return {"real": real, "dist": [list(d) for d in dist]}


def test_score_counts_what_it_says() -> None:
    cases = [
        case("fix", ("fix", 0.9), ("feat", 0.1)),
        case("feat", ("fix", 0.6), ("feat", 0.4)),
        case("docs", ("fix", 0.4), ("feat", 0.3), ("chore", 0.2), ("docs", 0.1)),
        case("fix"),                                              # no type at all
    ]
    s = score(cases)
    assert s["n"] == 4 and s["top1"] == 0.25 and s["top2"] == 0.5 and s["top3"] == 0.5
    assert [b["commits"] for b in s["bins"]] == [2, 1, 0, 1]       # <0.5 (0.4 and the empty one), 0.5-0.7, 0.7-0.9, >=0.9
    assert s["bins"][3]["right"] == 1.0 and s["bins"][2]["right"] is None
    ask = {a["below"]: a for a in s["asking"]}
    # Asking below 0.7 asks the three low ones (0.6, 0.4, 0.0) and the person fixes those with the type in the top three.
    assert ask[0.7]["asked"] == 0.75 and ask[0.7]["type_right"] == 0.5
    assert ask[0.5]["asked"] == 0.5 and ask[0.5]["type_right"] == 0.25
    # Brier of the first case: (1-0.9)^2 + 0.1^2 = 0.02, over the 11 types (the rest are 0)
    assert abs(score(cases[:1])["brier"] - 0.02) < 1e-9
    assert abs(score(cases[:1])["nll"] - (-math.log(0.9))) < 1e-9


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

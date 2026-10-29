"""Checks the measures of `type_confidence` on a few distributions made up for it (no server needed).

    python -I test_type_confidence.py
"""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from type_confidence import (  # noqa: E402
    auroc, conformal_level, ece, from_logprobs, listed_types, mass_above, menus, offer, prediction_set, ranked, score,
    sets, type_of,
)


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


def test_the_types_listed_in_the_prompt_are_the_allowed_ones() -> None:
    system = "Rules:\n- Types used here: fix, feat, titus, refactor.\n- Add a body"
    assert listed_types(system) == ["fix", "feat", "titus", "refactor"]
    assert listed_types("Rules:\n- nothing") == []


def test_the_menu_is_the_one_the_product_offers() -> None:
    unsure = [["feat", 0.45], ["fix", 0.35], ["refactor", 0.15], ["docs", 0.05]]
    assert offer(unsure, [], 0.5) == ["feat", "fix", "refactor", "docs"]
    assert offer([["fix", 0.92], ["feat", 0.08]], [], 0.5) is None            # sure enough
    assert offer([["feat", 0.5], ["fix", 0.5]], [], 0.5) is None              # exactly at the limit is sure
    assert offer([["feat", 0.5], ["revert", 0.3], ["fix", 0.2]], ["feat", "fix"], 0.8) == ["feat", "fix"]
    assert offer([["feat", 0.4]], [], 0.5) is None                            # nothing to choose between
    assert offer([], [], 0.5) is None
    many = [["feat", 0.3], ["fix", 0.25], ["docs", 0.2], ["chore", 0.15], ["refactor", 0.07], ["test", 0.03]]
    assert offer(many, [], 0.5) == ["feat", "fix", "docs", "chore"]            # at most four
    assert offer(many, [], 0.5, cap=3) == ["feat", "fix", "docs"]
    assert offer([["feat", 0.4], ["fix", 0.35], ["docs", 0.25]], [], 0.5, cap=1) is None     # one is nothing to choose


def test_an_adaptive_menu_offers_two_when_the_model_is_split_between_them() -> None:
    split = [["feat", 0.48], ["fix", 0.40], ["docs", 0.08], ["chore", 0.04]]
    assert offer(split, [], 0.5, adaptive=True) == ["feat", "fix"]
    assert offer(split, [], 0.5) == ["feat", "fix", "docs", "chore"]
    spread = [["feat", 0.4], ["fix", 0.3], ["docs", 0.2], ["chore", 0.1]]       # the two do not hold 0.85
    assert offer(spread, [], 0.5, adaptive=True) == ["feat", "fix", "docs", "chore"]
    apart = [["feat", 0.6], ["fix", 0.3], ["docs", 0.1]]                        # together 0.9, but 0.3 apart
    assert offer(apart, [], 0.7, adaptive=True) == ["feat", "fix", "docs"]
    wide = [["feat", 0.49], ["fix", 0.44], ["docs", 0.07]]                      # together 0.93, apart by 0.05: split
    assert offer(wide, [], 0.5, adaptive=True) == ["feat", "fix"]


def test_menus_count_the_asks_the_lines_and_the_type_right() -> None:
    cases = [
        {"real": "fix", "allowed": [], "dist": [["feat", 0.48], ["fix", 0.40], ["docs", 0.08], ["chore", 0.04]]},
        {"real": "chore", "allowed": [], "dist": [["feat", 0.48], ["fix", 0.40], ["docs", 0.08], ["chore", 0.04]]},
        {"real": "docs", "allowed": [], "dist": [["docs", 0.95], ["fix", 0.05]]},
        {"real": "fix", "allowed": [], "dist": [["docs", 0.95], ["fix", 0.05]]},
    ]
    row = next(r for r in menus(cases) if r["below"] == 0.5)
    assert row["present"] == {"asked": 0.5, "lines": 4.0, "type_right": 0.75}    # asked: both; sure: docs right, fix wrong
    assert row["adaptive"] == {"asked": 0.5, "lines": 2.0, "type_right": 0.5}    # the chore falls outside the two
    assert row["cap3"] == {"asked": 0.5, "lines": 3.0, "type_right": 0.5}        # and outside the three likeliest


def test_the_mass_above_the_real_type_counts_only_the_types_the_repository_uses() -> None:
    dist = [["feat", 0.5], ["fix", 0.3], ["docs", 0.2]]
    assert mass_above({"real": "feat", "dist": dist}) == 0.0
    assert abs(mass_above({"real": "fix", "dist": dist}) - 0.5) < 1e-9
    assert abs(mass_above({"real": "docs", "dist": dist}) - 0.8) < 1e-9
    assert mass_above({"real": "chore", "dist": dist}) == 1.0                     # not among them
    only = {"real": "fix", "dist": dist, "allowed": ["fix", "docs"]}              # feat is not used here
    assert mass_above(only) == 0.0 and abs(ranked(dist, ["fix", "docs"])[0][1] - 0.6) < 1e-9
    assert mass_above({"real": "fix", "dist": []}) == 1.0


def test_the_conformal_level_is_a_quantile_of_the_scores() -> None:
    # Nine commits whose real type has this mass above it (a two-type distribution gives 0 or the first's mass).
    masses = [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.8, 1.0]
    cases = [{"real": "b" if m else "a", "dist": [["a", m], ["b", 1 - m]]} for m in masses]
    assert conformal_level(cases, 0.1) == 1.0        # ceil(10 * 0.9) = 9th of 9
    assert conformal_level(cases, 0.2) == 0.8        # 8th
    assert conformal_level(cases, 0.5) == 0.5        # 5th
    assert conformal_level(cases, 0.99) == 0.0       # the 1st


def test_a_prediction_set_grows_with_the_level() -> None:
    case = {"real": "fix", "dist": [["feat", 0.5], ["fix", 0.3], ["docs", 0.2]]}
    assert prediction_set(case, 0.0) == ["feat"]
    assert prediction_set(case, 0.49) == ["feat"]
    assert prediction_set(case, 0.51) == ["feat", "fix"]
    assert prediction_set(case, 0.81) == ["feat", "fix", "docs"]
    assert prediction_set({"real": "x", "dist": []}, 1.0) == []


def test_sets_report_coverage_and_what_they_ask() -> None:
    sure = {"real": "fix", "dist": [["fix", 0.95], ["feat", 0.05]]}
    split = {"real": "feat", "dist": [["fix", 0.5], ["feat", 0.4], ["docs", 0.1]]}
    got = sets([sure, split, split], [sure, split], 0.4)                       # scores 0, 0.5, 0.5: level 0.5
    assert got["level"] == 0.5
    assert got["coverage"] == 1.0 and got["asked"] == 0.5 and got["lines"] == 2.0   # the split one asks with fix and feat
    low = sets([sure, sure, split], [sure, split], 0.5)                        # scores 0, 0, 0.5: level 0
    assert low["level"] == 0.0 and low["coverage"] == 0.5 and low["asked"] == 0.0 and low["lines"] == 0.0


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

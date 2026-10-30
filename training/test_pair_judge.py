"""Checks `pair_judge` on a few cases made up for it.

    python -I test_pair_judge.py
"""

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from pair_judge import (  # noqa: E402
    agreement, build, check, comparisons_for, interval, merge, net, norm, outcome, render_comparisons, side,
)


def test_the_side_is_the_same_for_the_same_comparison_and_balanced() -> None:
    assert side(29, "A", "c001-1") == side(29, "A", "c001-1")
    sides = [side(29, "A", f"c{n:03d}-1") for n in range(400)]
    assert 140 < sides.count("X") < 260 and set(sides) == {"X", "Y"}
    # another pass has sides of its own for the same comparisons
    assert any(side(29, "A", f"c{n:03d}-1") != side(29, "B", f"c{n:03d}-1") for n in range(50))


def test_a_case_has_a_comparison_for_each_other_candidate_and_identical_ones_are_ties() -> None:
    texts = {"author": "fix: handle HEAD", "m15": "fix: handle HEAD", "m05": "fix: read the branch name",
             "old": "Fix:  handle  head"}
    got = comparisons_for("c007", texts, "m15")
    assert [c["id"] for c in got] == ["c007-1", "c007-2", "c007-3"]
    assert [c["other"] for c in got] == ["author", "m05", "old"]
    assert [c["auto_tie"] for c in got] == [True, False, True]            # case and spaces do not make a message another
    assert norm("  Fix:  A   b ") == "fix: a b"


def test_the_comparisons_show_the_two_messages_on_sides_that_hide_their_origin() -> None:
    case = {"id": "c007", "comparisons": comparisons_for(
        "c007", {"author": "fix: A", "m15": "fix: B", "m05": "fix: B"}, "m15")}
    text = render_comparisons(case, 29, "A")
    assert "Comparison c007-1:" in text and "Comparison c007-2:" not in text      # the second is an auto tie
    focus_side = side(29, "A", "c007-1")
    assert (f"- {focus_side}: fix: B" in text) and (f"- {'Y' if focus_side == 'X' else 'X'}: fix: A" in text)
    assert "author" not in text and "m15" not in text


def test_an_answer_is_checked() -> None:
    ok = {"id": "c1-1", "winner": "X", "score_x": 4, "score_y": 2, "invented_x": False, "invented_y": True}
    assert check(ok, {"c1-1"}) == []
    assert check({**ok, "id": "c9-1"}, {"c1-1"}) and check({**ok, "winner": "x"}, {"c1-1"})
    assert check({**ok, "score_x": 6}, {"c1-1"}) and check({**ok, "score_y": 0}, {"c1-1"})
    assert check({**ok, "score_x": 4.5}, {"c1-1"}) and check({**ok, "invented_x": "no"}, {"c1-1"})
    assert check({**ok, "winner": "tie"}, {"c1-1"}) == []


def test_the_outcome_is_read_from_the_focus_models_side() -> None:
    a = {"winner": "X", "score_x": 5, "score_y": 2, "invented_x": False, "invented_y": True}
    assert outcome(a, "X") == {"outcome": "win", "score_focus": 5, "score_other": 2, "invented_focus": False,
                               "invented_other": True, "focus_side": "X"}
    assert outcome(a, "Y") == {"outcome": "loss", "score_focus": 2, "score_other": 5, "invented_focus": True,
                               "invented_other": False, "focus_side": "Y"}
    assert outcome({**a, "winner": "tie"}, "Y")["outcome"] == "tie"


def test_net_and_its_interval() -> None:
    o = ["win"] * 5 + ["loss"] * 2 + ["tie"] * 3
    assert abs(net(o) - 0.3) < 1e-9 and net([]) == 0.0
    mean, low, high = interval(o)
    assert abs(mean - 0.3) < 1e-9 and low < mean < high and -1 <= low and high <= 1
    assert interval(["win"] * 10) == (1.0, 1.0, 1.0)


def test_the_passes_agree_when_they_give_the_same_outcome() -> None:
    js = [
        {"id": "c1-1", "pass": "A", "outcome": "win"}, {"id": "c1-1", "pass": "B", "outcome": "win"},
        {"id": "c2-1", "pass": "A", "outcome": "win"}, {"id": "c2-1", "pass": "B", "outcome": "loss"},
        {"id": "c3-1", "pass": "A", "outcome": "tie"}, {"id": "c3-1", "pass": "B", "outcome": "win"},
        {"id": "c4-1", "pass": "A", "outcome": "tie", "auto_tie": True}, {"id": "c4-1", "pass": "B", "outcome": "tie", "auto_tie": True},
    ]
    a = agreement(js)
    assert a["compared"] == 3 and abs(a["same"] - 1 / 3) < 1e-9
    assert a["decided_both"] == 2 and a["same_when_decided"] == 0.5


def test_build_and_merge_end_to_end() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        audit, set_dir, out = root / "audit", root / "set", root / "out"
        audit.mkdir(), set_dir.mkdir()
        key = [{"id": f"c{n:03d}", "line": n, "real_subject": f"fix: A{n}"} for n in range(6)]
        (audit / "sample.json").write_text(json.dumps(key))
        (set_dir / "inputs.jsonl").write_text("".join(json.dumps({
            "subjects": ["fix: older"], "staged": [], "files": [{"path": f"f{n}.rs", "lines": [1, 0], "hunks": "@@ -0,0 +1 @@\n+x\n"}]
        }) + "\n" for n in range(6)))
        # the large model's message for case 0 is the author's: an identical pair no one is asked
        for name, texts in (("m_large", ["fix: A0"] + [f"fix: L{n}" for n in range(1, 6)]),
                            ("m_small", [f"fix: S{n}" for n in range(6)])):
            (root / f"{name}.jsonl").write_text("".join(json.dumps({"generation": {"message": {"subject": t}}}) + "\n" for t in texts))
        build(audit, set_dir, out, [("m_large", root / "m_large.jsonl"), ("m_small", root / "m_small.jsonl")], 29)
        text = "".join(p.read_text() for p in out.glob("P*.md")).lower()
        # the judge sees the messages, never who wrote them
        assert "author" not in text and "m_large" not in text and "m_small" not in text
        assert "fix: a1" in text and "fix: l1" in text and "fix: s1" in text
        answered = 0
        for pass_name in ("A", "B"):
            for number in range(1, 5):
                ids = (out / f"P{pass_name}-{number}.ids").read_text().split()
                answered += len(ids)
                (out / f"P{pass_name}-{number}.json").write_text(json.dumps([
                    {"id": i, "winner": "tie", "score_x": 3, "score_y": 3, "invented_x": False, "invented_y": False, "reason": "r"}
                    for i in ids]))
        assert answered == 2 * 11            # 6 cases x 2 others, less the identical pair (author 0 = large 0), in 2 passes
        merge(out)
        js = json.loads((out / "judgments.json").read_text())
        assert len(js) == 2 * 12 and sum(bool(j.get("auto_tie")) for j in js) == 2
        assert all(j["outcome"] == "tie" for j in js)


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

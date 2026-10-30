"""Checks `audit_types` on a few cases made up for it.

    python -I test_audit_types.py
"""

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from audit_types import (  # noqa: E402
    agreement, batches_for, build, check, cohen_kappa, fold, jaccard, judge, merge, render_case, sample_cases,
    score_model,
)

INPUTS = {
    "subjects": ["fix: handle HEAD", "feat(cli): add a flag", "update"],
    "staged": [{"status": "added", "path": "src/new.rs", "old_path": None}],
    "files": [
        {"path": "src/new.rs", "lines": [3, 0], "hunks": "@@ -0,0 +1,3 @@\n+fn new() {}\n"},
        {"path": "logo.png", "lines": None, "hunks": ""},
        {"path": "src/old.rs", "lines": [1, 1], "hunks": "@@ -1 +1 @@\n-a\n+b\n"},
    ],
}


def test_a_case_shows_the_history_and_the_changes_but_not_the_answer() -> None:
    text = render_case("c007", INPUTS)
    assert text.startswith("## Case c007")
    assert "- fix: handle HEAD" in text and "- added src/new.rs (+3 -0)" in text
    assert "- modified src/old.rs (+1 -1)" in text and "- modified logo.png (binary)" in text
    assert "--- src/new.rs\n@@ -0,0 +1,3 @@" in text and "+fn new() {}" in text and "```diff" in text
    assert "--- logo.png" not in text                                    # no hunks, no section


def test_a_long_diff_is_cut_by_file_and_in_total() -> None:
    big = {"subjects": [], "staged": [], "files": [
        {"path": f"f{n}.rs", "lines": [900, 0], "hunks": "@@ -0,0 +1 @@\n" + "+x\n" * 900} for n in range(10)]}
    text = render_case("c001", big)
    assert "... (cut)" in text and "- (none)" in text
    assert "later files not shown" in text and len(text) < 8000


def test_types_are_folded() -> None:
    assert fold("Tests") == "test" and fold(" FIX ") == "fix" and fold(None) is None and fold("") is None


def test_the_sample_is_the_same_for_the_same_seed_and_sorted() -> None:
    pool = list(range(0, 400, 2))
    a, b = sample_cases(pool, 20, 5), sample_cases(pool, 20, 5)
    assert a == b and a == sorted(a) and len(set(a)) == 20 and set(a) <= set(pool)
    assert sample_cases(pool, 20, 6) != a and len(sample_cases([1, 2, 3], 10, 5)) == 3


def test_each_annotator_reads_every_case_once_in_an_order_of_their_own() -> None:
    ids = [f"c{n:03d}" for n in range(100)]
    for annotator in ("A", "B"):
        batches = batches_for(ids, annotator, 17, 4)
        assert [len(b) for b in batches] == [25] * 4 and sorted(sum(batches, [])) == ids
    assert batches_for(ids, "A", 17, 4) != batches_for(ids, "B", 17, 4)
    assert batches_for(ids, "A", 17, 4) == batches_for(ids, "A", 17, 4)
    assert [len(b) for b in batches_for(ids[:10], "A", 1, 4)] == [3, 3, 2, 2]


def test_an_annotation_is_checked() -> None:
    ok = {"id": "c1", "acceptable": ["fix", "Tests"], "best": "fix"}
    assert check(ok, {"c1"}) == []
    assert check({**ok, "id": "c9"}, {"c1"}) and check({**ok, "acceptable": []}, {"c1"})
    assert check({**ok, "acceptable": ["fix", "bugfix"]}, {"c1"})
    assert check({**ok, "best": "feat"}, {"c1"})                              # best must be one of the accepted
    assert check({**ok, "best": "tests", "acceptable": ["test"]}, {"c1"}) == []


def test_agreement_measures() -> None:
    assert jaccard({"fix", "feat"}, {"fix"}) == 0.5 and jaccard(set(), set()) == 1.0 and jaccard({"a"}, {"b"}) == 0.0
    assert cohen_kappa(["a", "a", "b", "b"], ["a", "a", "b", "b"]) == 1.0
    assert abs(cohen_kappa(["a", "a", "b", "b"], ["a", "b", "a", "b"])) < 1e-9          # chance: observed 0.5, expected 0.5
    assert abs(cohen_kappa(["a", "a", "a", "b"], ["a", "a", "b", "b"]) - 0.5) < 1e-9      # observed 0.75, expected 0.5


def case(real: str, a: list[str], b: list[str], best_a: str | None = None, best_b: str | None = None) -> dict:
    return {"id": "c", "line": 0, "real_type": real,
            "A": {"acceptable": a, "best": best_a or a[0], "ambiguous": len(a) > 1, "reason": ""},
            "B": {"acceptable": b, "best": best_b or b[0], "ambiguous": len(b) > 1, "reason": ""}}


def test_a_type_is_judged_against_the_real_one_either_annotator_and_both() -> None:
    c = case("fix", ["feat", "fix"], ["fix", "refactor"])
    assert judge("fix", c) == {"strict": True, "lenient": True, "agreed": True}
    assert judge("feat", c) == {"strict": False, "lenient": True, "agreed": False}
    assert judge("docs", c) == {"strict": False, "lenient": False, "agreed": False}
    assert judge(None, c) == {"strict": False, "lenient": False, "agreed": False}
    apart = case("fix", ["feat"], ["refactor"])
    assert judge("feat", apart) == {"strict": False, "lenient": True, "agreed": None}   # they share none: no agreed answer


def test_the_report_numbers() -> None:
    cases = [case("fix", ["fix"], ["fix"]), case("feat", ["fix", "feat"], ["fix"]), case("docs", ["chore"], ["build"])]
    a = agreement(cases)
    assert a["n"] == 3 and abs(a["same_set"] - 1 / 3) < 1e-9
    assert abs(a["mean_jaccard"] - (1 + 0.5 + 0) / 3) < 1e-9 and abs(a["share_a_type"] - 2 / 3) < 1e-9
    assert abs(a["real_in_a"] - 2 / 3) < 1e-9 and abs(a["real_in_b"] - 1 / 3) < 1e-9
    assert abs(a["size_a"] - 4 / 3) < 1e-9 and abs(a["ambiguous_a"] - 1 / 3) < 1e-9
    s = score_model(["fix", "fix", "chore"], cases)
    # strict: fix=fix only; lenient: all three (the chore is A's); agreed: 2 cases share a type, both accept the fix
    assert abs(s["strict"] - 1 / 3) < 1e-9 and s["lenient"] == 1.0 and s["agreed"] == 1.0 and s["agreed_n"] == 2


def test_build_and_merge_end_to_end_without_leaking_the_answer() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        set_dir, out = root / "set", root / "out"
        set_dir.mkdir()
        metas, inputs = [], []
        for n in range(30):
            kind = "fix" if n % 3 else "feat"
            metas.append({"repo": f"o/r{n}", "hash": f"h{n}", "target": f"{kind}: secret subject {n}", "type": kind,
                          "set": "conventional" if n % 5 else "free"})
            inputs.append({"subjects": [f"{kind}: older"], "staged": [], "files": [
                {"path": f"src/f{n}.rs", "lines": [1, 0], "hunks": f"@@ -0,0 +1 @@\n+fn f{n}() {{}}\n"}]})
        (set_dir / "meta.jsonl").write_text("".join(json.dumps(m) + "\n" for m in metas))
        (set_dir / "inputs.jsonl").write_text("".join(json.dumps(i) + "\n" for i in inputs))
        build(set_dir, out, 10, 3, 2)
        key = json.loads((out / "sample.json").read_text())
        assert len(key) == 10 and all(metas[k["line"]]["set"] == "conventional" for k in key)
        text = "".join(p.read_text() for p in out.glob("*.md"))
        assert "secret subject" not in text and "o/r" not in text and "h1" not in text.replace("hunks", "")
        assert sorted(p.name for p in out.glob("*.md")) == ["A-1.md", "A-2.md", "B-1.md", "B-2.md"]
        for annotator in ("A", "B"):
            for number in (1, 2):
                ids = (out / f"{annotator}-{number}.ids").read_text().split()
                (out / f"{annotator}-{number}.json").write_text(json.dumps(
                    [{"id": i, "acceptable": ["fix", "feat"], "best": "fix", "ambiguous": True, "reason": "r"} for i in ids]))
        merge(out)
        audit = json.loads((out / "audit.json").read_text())
        assert len(audit) == 10 and audit[0]["A"]["acceptable"] == ["feat", "fix"] and audit[0]["real_type"] in ("fix", "feat")


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

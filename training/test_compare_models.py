"""Checks `compare_models` with a stand-in smart-commit.

    python -I test_compare_models.py
"""

import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from compare_models import better, habits, line_count, mean_subject_length, run_eval, table  # noqa: E402


def fake_smart_commit(d: Path) -> str:
    """Answers `eval INPUTS ...` with a trace a line, whose subject is the model's
    file name, and counts how many times it ran."""
    fake = d / "smart-commit"
    fake.write_text(
        f"#!{sys.executable}\n"
        "import json, os, sys\n"
        f"open({str(d / 'runs')!r}, 'a').write('x')\n"
        "name = os.path.basename(os.environ['SMART_COMMIT_GGUF'])\n"
        "for _ in open(sys.argv[2]):\n"
        "    print(json.dumps({'generation': {'message': {'subject': name}}}))\n"
    )
    fake.chmod(0o755)
    return str(fake)


def test_a_relative_model_path_is_made_absolute_and_a_failure_says_why() -> None:
    """smart-commit refuses a relative SMART_COMMIT_GGUF, and the script said nothing."""
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        inputs = d / "inputs.jsonl"
        inputs.write_text("{}\n")
        fake = d / "strict"
        fake.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "path = os.environ['SMART_COMMIT_GGUF']\n"
            "if not os.path.isabs(path):\n"
            "    sys.exit('error: expected an absolute path, got ' + path)\n"
            "print(json.dumps({'generation': {'message': {'subject': path}}}))\n"
        )
        fake.chmod(0o755)
        out = d / "out/model.jsonl"
        here = Path.cwd()
        os.chdir(d)
        try:
            assert run_eval(str(fake), "models/m.gguf", inputs, out, "full", "plain", "vulkan")
        finally:
            os.chdir(here)
        assert json.loads(out.read_text())["generation"]["message"]["subject"] == str(d / "models/m.gguf")
        # A model that cannot run leaves no trace behind and says what smart-commit said.
        broken = d / "broken"
        broken.write_text("#!/bin/sh\necho 'error: no such model' >&2\nexit 1\n")
        broken.chmod(0o755)
        gone = d / "out/gone.jsonl"
        try:
            run_eval(str(broken), "/x.gguf", inputs, gone, "full", "plain", "vulkan")
        except SystemExit as e:
            assert "no such model" in str(e) and "/x.gguf" in str(e)
        else:
            raise AssertionError("a failed evaluation went unnoticed")
        assert not gone.exists()


def test_a_flag_gained_is_better_for_a_good_metric_and_worse_for_a_bad_one() -> None:
    assert better("type_match", 9, 13) == -4 and better("first_try", 27, 3) == 24
    assert better("ungrounded", 0, 48) == 48 and better("fallback", 5, 0) == -5
    assert better("word_f1", 3, 1) == 0  # not a yes/no metric of either kind


def test_traces_are_made_once_and_the_model_is_the_one_asked_for() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        inputs = d / "inputs.jsonl"
        inputs.write_text("{}\n{}\n{}\n")
        binary = fake_smart_commit(d)
        out = d / "out/set/model.jsonl"
        assert run_eval(binary, "/models/abc.gguf", inputs, out, "full", "plain", "vulkan")
        assert line_count(out) == 3 and mean_subject_length(out) == len("abc.gguf")
        # Complete: not made again. Cut short, as a stopped run leaves it: made again.
        assert not run_eval(binary, "/models/abc.gguf", inputs, out, "full", "plain", "vulkan")
        out.write_text(out.read_text().splitlines()[0] + "\n")
        assert run_eval(binary, "/models/other.gguf", inputs, out, "full", "plain", "vulkan")
        assert mean_subject_length(out) == len("other.gguf")
        assert (d / "runs").read_text() == "xx"


def test_habits_show_a_model_that_stopped_writing_scopes_or_took_to_one_type() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        meta = [{"set": "conventional", "target": "fix(core): a"}, {"set": "conventional", "target": "tests: b"},
                {"set": "conventional", "target": "feat: c"}, {"set": "free", "target": "Add d"}]
        (d / "meta.jsonl").write_text("".join(json.dumps(m) + "\n" for m in meta))
        real = habits(d)
        assert real["conventional"] == {"n": 3, "scoped": 0.333, "types": {"fix": 1, "test": 1, "feat": 1}}
        assert real["free"]["n"] == 1 and real["free"]["types"] == {"-": 1}
        # A model that writes chore for everything, with no scope: scope_known would be 1.0.
        traces = d / "t.jsonl"
        traces.write_text("".join(json.dumps({"generation": {"message": {"subject": s}}}) + "\n"
                                  for s in ["chore: a", "chore: b", "chore: c", "Add d"]))
        got = habits(d, traces)["conventional"]
        assert got == {"n": 3, "scoped": 0.0, "types": {"chore": 3}}


def test_the_table_aligns_its_columns() -> None:
    text = table([["model", "a"], ["sft", "0.5"], ["longer", "10.25"]])
    assert text.splitlines() == ["model       a", "sft       0.5", "longer  10.25"]


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

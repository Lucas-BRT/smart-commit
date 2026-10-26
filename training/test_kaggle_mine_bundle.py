"""Checks the bundle of the Kaggle mining run.

    python -I test_kaggle_mine_bundle.py
"""

import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))
from kaggle_mine_bundle import LLAMA, REGISTRY, models, shards  # noqa: E402


def write_src(d: Path, count: int) -> None:
    d.mkdir()
    for name in ("inputs", "meta", "prompts"):
        (d / f"{name}.jsonl").write_text("".join(json.dumps({name: n}) + "\n" for n in range(count)))


def test_the_models_are_those_of_the_registry() -> None:
    pinned = models(REGISTRY.read_text())
    assert sorted(pinned) == ["0.5b", "1.5b"]
    for key, model in pinned.items():
        assert model["repo"] == f"LucasBRT/smart-commit-{key}-v0.1"
        assert model["file"] == f"smart-commit-{key}-v0.1-q4_k_m.gguf"
        assert re.fullmatch(r"[0-9a-f]{40}", model["revision"])
        assert re.fullmatch(r"[0-9a-f]{64}", model["sha256"]) and model["size"] > 10**8


def test_every_nth_input_goes_to_each_shard() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        write_src(d / "src", 10)
        parts = shards(d / "src", d / "out", 3)
        assert [p["inputs"] for p in parts] == [4, 3, 3]
        for k in range(3):
            for name in ("inputs", "meta", "prompts"):
                rows = [json.loads(line)[name] for line in (d / f"out/shard-{k}/{name}.jsonl").read_text().splitlines()]
                assert rows == list(range(k, 10, 3))
        # A source whose files disagree is refused.
        (d / "src/meta.jsonl").write_text("{}\n")
        try:
            shards(d / "src", d / "bad", 2)
        except SystemExit as e:
            assert "same number of lines" in str(e)
        else:
            raise AssertionError("files of different lengths were accepted")


def test_the_longest_jobs_come_first_and_everything_is_pinned() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        write_src(d / "src", 9)
        subprocess.run([sys.executable, "-I", str(HERE / "kaggle_mine_bundle.py"), str(d / "bundle"),
                        str(d / "src"), "--shards", "3", "--samples", "5"], check=True, capture_output=True)
        spec = json.loads((d / "bundle/runs.json").read_text())
        assert [j["name"] for j in spec["jobs"]] == [f"{m}-shard-{k}" for m in ("1.5b", "0.5b") for k in range(3)]
        assert spec["sampling"]["samples"] == 5 and spec["sampling"]["rules"] == "full"
        assert all(re.fullmatch(r"[0-9a-f]{64}", a["sha256"]) for a in spec["llama"]["assets"])
        assert spec["llama"] == LLAMA
        for name in ("mine_pairs.py", "dataset-metadata.json", "shard-0/prompts.jsonl"):
            assert (d / "bundle" / name).exists(), name
        # One model only.
        subprocess.run([sys.executable, "-I", str(HERE / "kaggle_mine_bundle.py"), str(d / "one"),
                        str(d / "src"), "0.5b"], check=True, capture_output=True)
        assert {j["model"] for j in json.loads((d / "one/runs.json").read_text())["jobs"]} == {"0.5b"}
        refused = subprocess.run([sys.executable, "-I", str(HERE / "kaggle_mine_bundle.py"), str(d / "x"),
                                  str(d / "src"), "7b"], capture_output=True, text=True)
        assert refused.returncode != 0 and "not in the registry" in refused.stderr


def test_the_notebook_fails_when_a_job_does() -> None:
    """A kernel whose jobs failed still says COMPLETE: only an exception makes it ERROR."""
    cells = json.loads((HERE / "kaggle_mine.ipynb").read_text())["cells"]
    scheduler = next("".join(c["source"]) for c in cells if "pending, running, attempts" in "".join(c["source"]))
    assert "assert not failed" in scheduler.splitlines()[-1]


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

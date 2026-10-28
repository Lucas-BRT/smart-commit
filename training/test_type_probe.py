"""Checks the features and the labels of `type_probe` (no scikit-learn needed).

    python -I test_type_probe.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from type_probe import bucket, by_file_tokens, conventional_type, tokens, type_of  # noqa: E402


def test_types_are_read_like_the_models_are_scored() -> None:
    assert type_of("fix(core)!: stop it") == "fix"
    assert type_of("Feat: Add") == "feat"
    assert type_of("add a thing") is None
    assert type_of(None) is None
    assert conventional_type({"set": "conventional", "type": "Perf"}) == "perf"
    assert conventional_type({"set": "free", "type": "fix"}) is None
    assert conventional_type({"set": "conventional", "type": None}) is None


def test_buckets_are_log_steps() -> None:
    assert [bucket(n) for n in (0, 1, 2, 3, 4, 100)] == [0, 1, 2, 2, 3, 7]
    assert bucket(-5) == 0


def test_a_commit_is_its_paths_status_words_and_history() -> None:
    inputs = {
        "staged": [{"path": "src/a.rs", "status": "added"}],
        "files": [
            {"path": "src/a.rs", "lines": [3, 1], "hunks": "@@ -1 +1 @@\n-let old_name = 1;\n+let new_name = 2;\n+++ b\n"},
            {"path": "README.md", "lines": [0, 0], "hunks": ""},
        ],
        "subjects": ["fix: a", "feat(x): b", "fix: c", "update"],
    }
    got = tokens(inputs)
    for feature in (
        "nfiles:2", "top:src", "dir:src", "name:a.rs", "ext:rs", "p:src", "status:added",
        "add:2", "del:1", "+new_name", "-old_name", "top:README.md", "dir:", "status:modified",
    ):
        assert feature in got, feature
    # The `+++` header is not an added line; history counts, capped at 5, and what is no type is left out.
    assert "++" not in "".join(got) and got.count("hist:fix") == 2 and got.count("hist:feat") == 1
    assert not any(t.startswith("hist:update") for t in got)
    many = {"files": [], "subjects": ["fix: x"] * 9}
    assert tokens(many).count("hist:fix") == 5


def test_the_history_by_file_tells_same_files_from_the_same_directory() -> None:
    inputs = {
        "subjects": ["docs: a", "fix: b", "update c", "feat: d", "fix: e"],
        "history_files": [["src/x.rs"], ["README.md"], ["src/a.rs"], ["src/a.rs"], ["lib/z.rs"]],
    }
    got = by_file_tokens(inputs, ["src/a.rs"])
    # `docs` touched another file of `src/`, `fix: b` another directory, `update c` is no type,
    # `feat: d` touched the file, `fix: e` another directory.
    assert got == ["near:docs", "same:feat", "latest:feat"]
    assert by_file_tokens({"subjects": ["fix: a"]}, ["a"]) == []
    assert not any(t.startswith("same:") for t in tokens({"files": [], "subjects": ["fix: a"]}, by_file=True))


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

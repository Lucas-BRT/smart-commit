"""Checks the scenarios and their builder. With a smart-commit binary, also
builds every case twice and requires identical files.

    python -I test_scenarios.py [SMART_COMMIT]
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from scenarios import EVAL_ONLY_TAGS, Case, Git, Repo, ScenarioError, apply, build_repo  # noqa: E402
from scenarios import expand, expected, load_cases, load_repos, meta_of, run_op, validate  # noqa: E402
from score import groups  # noqa: E402

HERE = Path(__file__).parent


def raises(fn, text: str) -> None:
    try:
        fn()
    except ScenarioError as e:
        assert text in str(e), (text, str(e))
        return
    raise AssertionError(f"no error with {text!r}")


def case(**fields) -> Case:
    base = {"id": "c", "title": "t", "repo": "r", "level": 0, "split": "eval",
            "target": "x", "ops": []}
    return Case(**{**base, **fields})


def tiny_repo(tmp: Path) -> Repo:
    files = tmp / "files"
    (files / "src").mkdir(parents=True)
    (files / "src" / "a.py").write_text("x = 1\ny = 2\n")
    (files / "README.md").write_text("# tiny\n")
    commits = [{"message": "commit inicial", "paths": ["README.md"]},
               {"message": "adiciona a.py", "paths": ["src/a.py"]},
               {"message": "muda y", "ops": [{"op": "replace", "path": "src/a.py", "old": "y = 2", "new": "y = 3"}]}]
    return Repo("tiny", "free", commits, files)


def test_shipped_cases_are_valid() -> None:
    repos = load_repos()
    cases = load_cases(repos=repos)
    assert {"cafeteria", "ledger"} <= set(repos)
    assert len(cases) >= 80
    for split in ("train", "eval"):
        assert {c.level for c in cases if c.split == split} == set(range(6)), split
    for c in cases:
        assert not (c.split == "train" and EVAL_ONLY_TAGS & set(c.tags)), c.id


def test_variants() -> None:
    raw = {"id": "v", "title": "t", "repo": "r", "split": "train", "target": "set {{name}}",
           "ops": [{"op": "write", "path": "{{name}}.txt", "text": "`${keep}`"}],
           "variants": [{"name": "one", "vars": {"name": "a"}},
                        {"vars": {"name": "b"}, "split": "eval", "target": "other"}]}
    one, two = expand(raw, {"level": 3})
    assert (one.id, one.split, one.target, one.level) == ("v/one", "train", "set a", 3)
    assert one.ops[0] == {"op": "write", "path": "a.txt", "text": "`${keep}`"}
    assert (two.id, two.split, two.target) == ("v/2", "eval", "other")
    # without variants nothing is substituted: code keeps its braces
    plain, = expand({**raw, "variants": None, "target": "{{literal}}"}, {"level": 0})
    assert plain.target == "{{literal}}" and plain.id == "v"
    raises(lambda: expand({**raw, "variants": [{"vars": {"other": 1}}]}, {"level": 0}),
           "unknown variable")
    raises(lambda: expand({**raw, "bogus": 1}, {"level": 0}), "case v")


def test_validation() -> None:
    repos = {"r": None}
    validate([case()], repos)
    raises(lambda: validate([case(), case()], repos), "duplicate id")
    raises(lambda: validate([case(repo="nope")], repos), "unknown repo")
    raises(lambda: validate([case(split="test")], repos), "split must be")
    raises(lambda: validate([case(split="train", tags=["git-state"])], repos), "eval only")
    raises(lambda: validate([case(target="")], repos), "no target")
    validate([case(target="", outcome="error")], repos)
    raises(lambda: validate([case(ops=[{"op": "teleport"}])], repos), "unknown op")


def test_repo_history_is_deterministic() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        repo = tiny_repo(Path(tmp))
        heads = []
        for n in range(2):
            git = build_repo(repo, Path(tmp) / f"r{n}")
            heads.append(git("rev-parse", "HEAD").strip())
            assert git("log", "--format=%s").split("\n")[:3] == ["muda y", "adiciona a.py", "commit inicial"]
            assert (Path(tmp) / f"r{n}" / "src" / "a.py").read_text() == "x = 1\ny = 3\n"
        assert heads[0] == heads[1]
        fresh = build_repo(repo, Path(tmp) / "fresh", history=False)
        assert fresh("status", "--short").split() == ["??", "README.md", "??", "src/"]
        assert (Path(tmp) / "fresh" / "src" / "a.py").read_text() == "x = 1\ny = 3\n"
        (repo.files / "stray.txt").write_text("")
        raises(lambda: build_repo(repo, Path(tmp) / "r2"), "stray.txt")


def test_operations() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        git = Git(root)
        git("init", "-q", "-b", "main")
        run_op({"op": "write", "path": "big.txt", "text": "l{{i}}\n", "repeat": 3}, root, git)
        assert (root / "big.txt").read_text() == "l0\nl1\nl2\n"
        run_op({"op": "append", "path": "big.txt", "text": "end\n"}, root, git)
        run_op({"op": "prepend", "paths": ["*.txt"], "text": "# h\n"}, root, git)
        assert (root / "big.txt").read_text() == "# h\nl0\nl1\nl2\nend\n"
        raises(lambda: run_op({"op": "replace", "path": "big.txt", "old": "l", "new": "L"}, root, git),
               "3 occurrences")
        run_op({"op": "replace", "path": "big.txt", "old": "l", "new": "L", "all": True}, root, git)
        run_op({"op": "regex", "path": "big.txt", "pattern": "^L(\\d)$", "repl": "n\\1"}, root, git)
        assert (root / "big.txt").read_text() == "# h\nn0\nn1\nn2\nend\n"
        raises(lambda: run_op({"op": "regex", "path": "big.txt", "pattern": "zzz", "repl": ""}, root, git),
               "no match")
        run_op({"op": "regex", "path": "big.txt", "pattern": "\n", "repl": "\r\n"}, root, git)
        assert (root / "big.txt").read_bytes().count(b"\r\n") == 5
        run_op({"op": "binary", "path": "b/x.png", "size": 64, "seed": 1}, root, git)
        first = (root / "b" / "x.png").read_bytes()
        run_op({"op": "binary", "path": "b/x.png", "size": 64, "seed": 1}, root, git)
        assert len(first) == 64 and (root / "b" / "x.png").read_bytes() == first
        raises(lambda: run_op({"op": "write", "path": "../escape", "text": ""}, root, git), "outside")
        raises(lambda: run_op({"op": "append", "path": "none/*.py", "text": ""}, root, git), "no file matches")
        git("add", "-A")
        git("commit", "-q", "-m", "um assunto único")
        run_op({"op": "git", "args": ["tag", "t", "{{commit:assunto único}}"]}, root, git)
        assert git("rev-parse", "t").strip() == git("rev-parse", "HEAD").strip()
        raises(lambda: run_op({"op": "git", "args": ["tag", "u", "{{commit:nenhum}}"]}, root, git),
               "0 commits")


def test_apply_stages_what_the_case_says() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        repo = tiny_repo(Path(tmp))
        staged_all = case(repo="tiny", ops=[{"op": "write", "path": "new.py", "text": "z = 0\n"}])
        git = apply(staged_all, repo, Path(tmp) / "a")
        assert git("diff", "--cached", "--name-only").split() == ["new.py"]
        partial = case(repo="tiny", stage="none", ops=[
            {"op": "replace", "path": "src/a.py", "old": "x = 1", "new": "x = 10"},
            {"op": "git", "args": ["add", "-A"]},
            {"op": "replace", "path": "README.md", "old": "tiny", "new": "small"},
        ])
        git = apply(partial, repo, Path(tmp) / "b")
        assert git("diff", "--cached", "--name-only").split() == ["src/a.py"]
        assert git("diff", "--name-only").split() == ["README.md"]
        broken = case(repo="tiny", ops=[{"op": "replace", "path": "src/a.py", "old": "q", "new": "r"}])
        raises(lambda: apply(broken, repo, Path(tmp) / "c"), "case c, op 1 (replace)")


def test_expected_and_meta() -> None:
    assert expected("Corrige o PREÇO do café", ["preco", "cafe|cha"]) is True
    assert expected("corrige o preço", ["preco", "cafe|cha"]) is False
    assert expected("anything", []) is None
    repo = Repo("ledger", "conventional", [], Path("."))
    inputs = {"subjects": ["a", "b"]}
    m = meta_of(case(repo="ledger", target="feat(cli): add x", level=3, tags=["feature"]), repo, inputs)
    assert (m["type"], m["set"], m["history"], m["level"], m["hash"]) == ("feat", "conventional", 2, 3, "c")
    m = meta_of(case(repo="ledger", target="chore: initial commit", history=False), repo, {"subjects": []})
    assert m["set"] == "free"
    m = meta_of(case(repo="ledger", target='Revert "feat: x"'), repo, inputs)
    assert (m["type"], m["set"]) == ("revert", "conventional")
    assert groups({"set": "free"}) == ["free", "all"]
    assert groups({"set": "free", "level": 2}) == ["free", "all", "level 2"]


def build_all(binary: str, out: Path) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, "-I", str(HERE / "build_scenarios.py"), "build", str(out), binary],
                          capture_output=True, text=True)


def check_build(binary: str) -> None:
    """Every case builds, twice to the same bytes, and lines up with its meta."""
    with tempfile.TemporaryDirectory() as tmp:
        first, second = Path(tmp) / "a", Path(tmp) / "b"
        for out in (first, second):
            result = build_all(binary, out)
            assert result.returncode == 0, result.stderr
        for name in ("inputs.jsonl", "meta.jsonl"):
            assert (first / name).read_bytes() == (second / name).read_bytes(), name
        inputs = [json.loads(l) for l in (first / "inputs.jsonl").read_text().splitlines()]
        meta = [json.loads(l) for l in (first / "meta.jsonl").read_text().splitlines()]
        cases = {c.id: c for c in load_cases()}
        assert len(inputs) == len(meta) == sum(c.outcome == "message" and not c.issue for c in cases.values())
        for i, m in zip(inputs, meta):
            # A merge resolved to HEAD stages nothing; git prepared its message.
            assert i["staged"] or i.get("prepared"), m["hash"]
            assert m["target"] == cases[m["hash"]].target
    print("ok build (twice, identical)")


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)
    if len(sys.argv) > 1:
        check_build(sys.argv[1])

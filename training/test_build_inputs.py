"""Checks `trim_context` against git itself: trimming git's `-U3` diff must
give git's `-U1` diff (headings aside). Seeded, so always the same cases.

    python -I test_build_inputs.py
"""

import collections
import json
import os
import random
import re
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from build_inputs import inputs_of, keep_target, trim_context  # noqa: E402
from build_inputs import paths_of  # noqa: E402
from score import area, description, scope_fits, specifics, ungrounded, unsupported  # noqa: E402
from make_examples import make_examples  # noqa: E402
from build_inputs import conventional_type, release_version, version_in_diff  # noqa: E402
from score import per_type  # noqa: E402

GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}


def git_diff(old: str, new: str, context: int, d: Path) -> str:
    (d / "a").write_text(old)
    (d / "b").write_text(new)
    out = subprocess.run(
        ["git", "diff", "--no-index", f"-U{context}", "a", "b"],
        cwd=d, env=GIT_ENV, capture_output=True, text=True,
    ).stdout
    return out[out.index("@@") :] if "@@" in out else ""


def strip_headings(diff: str) -> str:
    return re.sub(r"^(@@ [^@]* @@).*$", r"\1", diff, flags=re.M)


def mutate(rng: random.Random, lines: list[str]) -> list[str]:
    out = list(lines)
    for _ in range(rng.randint(1, 6)):
        op = rng.choice("adr")
        at = rng.randint(0, len(out))
        if op == "a":
            out[at:at] = [f"new {rng.random():.6f}" for _ in range(rng.randint(1, 3))]
        elif op == "d" and out:
            del out[at : at + rng.randint(1, 3)]
        elif out:
            out[min(at, len(out) - 1)] = f"changed {rng.random():.6f}"
    return out


def test_trim_matches_git() -> None:
    rng = random.Random(1)
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        for case in range(300):
            old = [f"line {i}" for i in range(rng.randint(0, 40))]
            new = mutate(rng, old)
            end = "" if case % 7 == 0 else "\n"  # some without a final newline
            o, n = "\n".join(old) + end, "\n".join(new) + end
            if not old:
                o = ""
            u3, u1 = git_diff(o, n, 3, d), git_diff(o, n, 1, d)
            got = strip_headings(trim_context(u3))
            assert got == strip_headings(u1), f"case {case}:\n{u3}\n--- want\n{u1}\n--- got\n{got}"


def test_targets() -> None:
    assert keep_target("feat(diff): parse renames")
    assert keep_target("chore: bump vue to 2.6.13")
    for bad in ["Merge branch 'x'", "Revert \"x\"", "fixup! feat: x", "WIP on parser",
                "chore: release 1.2.0", "v1.2.3", "chore(release): 0.3.8",
                "chore(publish): 5.5.0-beta.6", "Release v2.0.0", "bump 1.2.3", "#123", "fix #12", "short", "x" * 73]:
        assert not keep_target(bad), bad


def test_inputs() -> None:
    mods = [
        {"change_type": "RENAME", "old_path": "a.rs", "new_path": "b.rs", "diff": ""},
        {"change_type": "DELETE", "old_path": "c.rs", "new_path": None,
         "diff": "@@ -1 +0,0 @@\n-x\n"},
        {"change_type": "ADD", "old_path": None, "new_path": "img.png", "diff": ""},
    ]
    got = inputs_of(mods)
    assert [s["status"] for s in got["staged"]] == ["renamed", "deleted", "added"]
    assert got["staged"][0]["old_path"] == "a.rs"
    assert got["files"][0]["lines"] == [0, 0]
    assert got["files"][1] == {"path": "c.rs", "old_path": None, "lines": [0, 1],
                               "hunks": "@@ -1 +0,0 @@\n-x\n"}
    assert got["files"][2]["lines"] is None
    assert inputs_of([]) is None
    assert inputs_of([{"change_type": "UNKNOWN", "old_path": "a", "new_path": "a", "diff": ""}]) is None


def test_faithfulness() -> None:
    assert description("feat(cli): add `eval`") == "add `eval`"
    assert description("✨ add login page") == "add login page"
    assert specifics("bump to v5.5.0-beta.6 for PR 1234, use `foo` in fooBar and my_var") == [
        "v5.5.0-beta.6", "1234", "foo", "fooBar", "my_var"]
    known = "src/combinelatest.ts\n+  const foobar = 1\n"  # evidence() lowercases
    assert not unsupported("fix(rx): rename fooBar", known)
    assert unsupported("chore(release): 1.1.0", known)
    assert not unsupported("fix: handle empty input", known)
    assert ungrounded("fix: fix typo", known)
    assert ungrounded("docs: update docs", known)
    assert not ungrounded("fix(rx): guard combineLatest against empty arrays", known)


def test_scope_fits() -> None:
    assert area("src/diff.rs") == "src/" and area("mise.toml") == ""
    inputs = {
        "subjects": ["feat(training): add train.py", "fix(cli): parse flags", "docs: readme"],
        "history_files": [["training/train.py"], ["src/main.rs"], ["README.md"]],
        "files": [{"path": "mise.toml"}],
    }
    assert not scope_fits("feat(training): rust 1.99", inputs)
    assert scope_fits("chore: update rust in mise.toml", inputs)
    inputs["files"] = [{"path": "src/main.rs"}, {"path": "mise.toml"}]
    assert scope_fits("fix(cli): handle -C", inputs)
    assert not scope_fits("fix: handle -C", inputs)  # a scope fits but none given
    assert not scope_fits("fix(api): handle -C", inputs)  # never used
    assert scope_fits("fix: x", {"subjects": ["a"], "files": []}) is None


def test_paths_of() -> None:
    mods = [
        {"change_type": "DELETE", "old_path": "b.rs", "new_path": None},
        {"change_type": "RENAME", "old_path": "c.rs", "new_path": "a.rs"},
        {"change_type": "MODIFY", "old_path": "a.rs", "new_path": "a.rs"},
    ]
    assert paths_of(mods) == ["a.rs", "b.rs"]
    assert paths_of([{"change_type": "ADD", "old_path": None, "new_path": f"f{i:02}"}
                     for i in range(30)]) == [f"f{i:02}" for i in range(20)]


def test_filter_supported() -> None:
    commits = [
        ({"target": "feat(cli): add retry", "set": "conventional"}, "+fn retry() {}\n"),
        ({"target": "chore(release): 1.2.3", "set": "conventional"}, "+x\n"),
        ({"target": "fix: bump vue to 2.6.13", "set": "free"}, "+x\n"),
        ({"target": "fix: bump vue to 2.6.13", "set": "free"}, '+"vue": "2.6.13"\n'),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        src, out = Path(tmp) / "src", Path(tmp) / "out"
        src.mkdir()
        inputs = [json.dumps({"subjects": [], "files": [{"path": "a", "hunks": h}]})
                  for _, h in commits]
        (src / "inputs.jsonl").write_text("".join(i + "\n" for i in inputs))
        (src / "meta.jsonl").write_text("".join(json.dumps(m) + "\n" for m, _ in commits))
        subprocess.run([sys.executable, "-I", str(Path(__file__).parent / "filter_supported.py"),
                        str(src), str(out)], check=True, capture_output=True)
        kept = (out / "inputs.jsonl").read_text().splitlines()
        assert kept == [inputs[0], inputs[3]]
        assert len((out / "meta.jsonl").read_text().splitlines()) == 2


def test_make_examples_passes_the_rules() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        # A stand-in for smart-commit that answers with its own arguments.
        fake = d / "smart-commit"
        fake.write_text('#!/bin/sh\necho "{\\"system\\": \\"$*\\", \\"user\\": \\"u\\"}"\n')
        fake.chmod(0o755)
        (d / "inputs.jsonl").write_text("{}\n")
        meta = {"target": "feat: add x.", "repo": "r", "hash": "h", "set": "conventional"}
        (d / "meta.jsonl").write_text(json.dumps(meta) + "\n")
        for rules in ("full", "omitted"):
            make_examples(d, str(fake), rules)
            example = json.loads((d / "examples.jsonl").read_text())
            assert example["system"] == f"prompt --inputs {d / 'inputs.jsonl'} --rules {rules}"
            assert example["target"] == "feat: add x" and example["user"] == "u"


def test_type_scores() -> None:
    assert conventional_type("tests(cli): cover -C") == "test"
    assert conventional_type("test: x") == "test" and conventional_type("Fix: x") is None
    pairs = collections.Counter({("fix", "fix"): 3, ("fix", "feat"): 1, ("feat", "feat"): 1,
                                 ("docs", None): 2})
    scores, macro = per_type(pairs)
    assert list(scores) == ["fix", "docs", "feat"]
    assert scores["fix"] == {"n": 4, "precision": 1.0, "recall": 0.75, "f1": 0.857}
    assert scores["feat"] == {"n": 1, "precision": 0.5, "recall": 1.0, "f1": 0.667}
    assert scores["docs"] == {"n": 2, "precision": 0.0, "recall": 0.0, "f1": 0.0}
    assert macro == round((0.857 + 0.667 + 0.0) / 3, 3)
    assert per_type(collections.Counter()) == ({}, 0.0)


def test_releases() -> None:
    assert release_version("chore: release 0.5.0") == "0.5.0"
    assert release_version("Version bump to v2.3.0-dev") == "2.3.0-dev"
    assert release_version("feat: support 1.2 files") is None
    assert release_version("bump version") is None
    changes = {"files": [{"hunks": '@@ -1 +1 @@\n-version = "0.4.0"\n+version = "0.5.0"\n'},
                         {"hunks": None}]}
    assert version_in_diff("0.5.0", changes)
    assert not version_in_diff("0.4.0", changes)  # removed, not added
    assert not version_in_diff("5.0", changes)  # part of another version
    # select.py drops release bumps unless told its source holds them.
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "src"
        src.mkdir()
        meta = [{"repo": "r", "hash": h, "target": t, "type": "chore", "history": 10}
                for h, t in (("a", "chore: release 0.5.0"), ("b", "chore: release 0.6.0"))]
        (src / "meta.jsonl").write_text("".join(json.dumps(m) + "\n" for m in meta))
        (src / "inputs.jsonl").write_text("{}\n{}\n")
        for flags, kept in (([], 0), (["--releases"], 2)):
            out = Path(tmp) / f"out{kept}"
            subprocess.run([sys.executable, "-I", str(Path(__file__).parent / "select.py"),
                            str(src), str(out), "--conventional", "2", "--free", "0", *flags],
                           check=True, capture_output=True)
            assert len((out / "meta.jsonl").read_text().splitlines()) == kept


def test_score_reads_rule_messages() -> None:
    """A message from rules (a lockfile) has no attempt; score.py crashed on it."""
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        meta = {"repo": "r", "hash": "h", "target": "chore(deps): update lockfile", "type": "chore",
                "history": 1, "set": "conventional"}
        trace = {"inputs": {"branch": "main", "subjects": ["a"], "staged": [], "files": []},
                 "generation": {"message": {"subject": "chore(deps): update package-lock.json", "body": None},
                                "source": "rules", "attempts": []}}
        (d / "meta.jsonl").write_text(json.dumps(meta) + "\n")
        (d / "traces.jsonl").write_text(json.dumps(trace) + "\n")
        result = subprocess.run([sys.executable, "-I", str(Path(__file__).parent / "score.py"), str(d),
                                 str(d / "traces.jsonl"), "--json"], capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout)["all"]["first_try"] == 1.0


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

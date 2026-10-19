"""Checks `trim_context` against git itself: trimming git's `-U3` diff must
give git's `-U1` diff (headings aside). Seeded, so always the same cases.

    python -I test_build_inputs.py
"""

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


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

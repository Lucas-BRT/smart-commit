"""Checks `trim_context` against git itself: trimming git's `-U3` diff must
give git's `-U1` diff (headings aside). Seeded, so always the same cases.

    python -I test_build_inputs.py
"""

import os
import random
import re
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from build_inputs import inputs_of, keep_target, trim_context  # noqa: E402

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
    for bad in ["Merge branch 'x'", "Revert \"x\"", "fixup! feat: x", "WIP on parser",
                "chore: release 1.2.0", "v1.2.3", "#123", "fix #12", "short", "x" * 73]:
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


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

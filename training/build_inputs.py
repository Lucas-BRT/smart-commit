"""CommitChronicle parquet shards -> smart-commit trace `Inputs` + targets.

Writes two aligned JSON Lines files: `inputs.jsonl` (one `trace::Inputs` per
line, for `smart-commit prompt --inputs`) and `meta.jsonl` (repository, hash,
target subject and its Conventional Commits type). The prompt itself is built
by smart-commit, never here.

Deterministic: the same shards give byte-identical outputs.

    python -I build_inputs.py OUT_DIR SHARD.parquet [SHARD.parquet...] [--releases]

`--releases` keeps, instead, only the release bumps the target filters drop
whose version is on an added line of the diff, where the subject can be
told from the change (H7).
"""

import argparse
import json
import re
import sys
from datetime import datetime
from pathlib import Path

import pyarrow.parquet as pq

HISTORY_DEPTH = 30  # style.history_depth
SUBJECT_MIN, SUBJECT_MAX = 8, 72
CONTEXT = 1  # smart-commit runs `git diff -U1`
MAX_FILES = 64
MAX_DIFF_BYTES = 64 * 1024  # per file; the prompt keeps far less
HISTORY_FILES = 20  # paths kept per history commit, like git::Repo::recent_commits

STATUS = {
    "ADD": "added",
    "MODIFY": "modified",
    "DELETE": "deleted",
    "RENAME": "renamed",
    "COPY": "copied",
}

TYPES = "feat|fix|docs|style|refactor|perf|test|tests|build|ci|chore|revert"
CONVENTIONAL = re.compile(rf"^(?P<type>{TYPES})(\([^()]+\))?!?: \S")
# Spellings of one type, folded for selection and scoring; the target keeps
# the repository's own spelling, which the model should follow.
TYPE_ALIASES = {"tests": "test"}
RELEASE = [
    re.compile(p, re.IGNORECASE)
    for p in (
        r"^(chore(\([^)]*\))?: )?(release|bump version|version bump|prepare release)\b",
        r"^v?\d+(\.\d+)+\S*$",
        # A description that is only a version: "chore(release): 0.3.8".
        r"^\S+?(\([^)]*\))?!?:\s*(release\s+|version\s+)?v?\d+(\.\d+)+\S*$",
        r"^(release|version|bump)\s+v?\d+(\.\d+)+\S*$",
    )
]
DROP = [
    re.compile(p, re.IGNORECASE)
    for p in (
        r"^merge\b",
        r"^revert\b",
        r"^(fixup|squash|amend)!",
        r"\bwip\b",
    )
] + RELEASE + [
    re.compile(p, re.IGNORECASE)
    for p in (
        r"^(\W*(#|gh-)\d+\W*)+$",
        r"^(update|fix|close[sd]?|resolve[sd]?)\s+#\d+$",
    )
]
VERSION = re.compile(r"\bv?(\d+(?:\.\d+)+(?:[-+][\w.]+)?)")
HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$")


def subject_of(message: str) -> str:
    return message.strip().split("\n", 1)[0].strip()


def keep_target(subject: str) -> bool:
    return SUBJECT_MIN <= len(subject) <= SUBJECT_MAX and not any(
        p.search(subject) for p in DROP
    )


def release_version(subject: str) -> str | None:
    """The version a release bump names ("chore: release 1.2.0" -> "1.2.0"),
    or None for any other subject, or one without a version."""
    if not any(p.search(subject) for p in RELEASE):
        return None
    m = VERSION.search(subject)
    return m.group(1) if m else None


def version_in_diff(version: str, changes: dict) -> bool:
    """Whether `version` is on an added line of the diff `inputs_of` gave."""
    pattern = re.compile(rf"(?<![\w.]){re.escape(version)}(?![\w])")
    return any(
        line.startswith("+") and pattern.search(line)
        for f in changes["files"]
        for line in (f["hunks"] or "").splitlines()
    )


def conventional_type(subject: str) -> str | None:
    m = CONVENTIONAL.match(subject)
    return TYPE_ALIASES.get(m.group("type"), m.group("type")) if m else None


def date_key(date: str) -> datetime:
    return datetime.strptime(date, "%d.%m.%Y %H:%M:%S")


def range_header(start: int, count: int) -> str:
    # git: "-a" when one line, "-a,0" with a = the line before when none.
    return str(start) if count == 1 else f"{start},{count}"


def trim_context(diff: str, context: int = CONTEXT) -> str:
    """Re-hunks a unified diff to `context` lines of context, as `-U1` would.

    Hunks split where more than 2 * context unchanged lines separate two
    changes. Only the first piece of a split hunk keeps git's function
    heading, since the others' headings cannot be computed here."""
    out = []
    lines = diff.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    i = 0
    while i < len(lines):
        m = HUNK.match(lines[i])
        if not m:
            i += 1
            continue
        # An empty range names the line before it.
        old = int(m.group(1)) + (m.group(2) == "0")
        new = int(m.group(3)) + (m.group(4) == "0")
        heading = m.group(5)
        body = []
        i += 1
        while i < len(lines) and not lines[i].startswith("@@ "):
            body.append(lines[i])
            i += 1
        # Attach "\ No newline at end of file" to the line before it.
        entries: list[list[str]] = []
        for line in body:
            if line.startswith("\\") and entries:
                entries[-1].append(line)
            elif line[:1] in (" ", "-", "+"):
                entries.append([line])
            elif line == "":
                entries.append([" "])  # an empty context line lost its space
        changed = [e[0][0] != " " for e in entries]
        if not any(changed):
            continue
        keep = [False] * len(entries)
        for j, c in enumerate(changed):
            if c:
                for k in range(max(0, j - context), min(len(entries), j + context + 1)):
                    keep[k] = True
        # Line numbers before each entry.
        o, n = old, new
        pos = []
        for e in entries:
            pos.append((o, n))
            kind = e[0][0]
            if kind != "+":
                o += 1
            if kind != "-":
                n += 1
        first = True
        j = 0
        while j < len(entries):
            if not keep[j]:
                j += 1
                continue
            start = j
            while j < len(entries) and keep[j]:
                j += 1
            piece = entries[start:j]
            o0, n0 = pos[start]
            oc = sum(1 for e in piece if e[0][0] != "+")
            nc = sum(1 for e in piece if e[0][0] != "-")
            ostart = o0 if oc else o0 - 1
            nstart = n0 if nc else n0 - 1
            out.append(
                f"@@ -{range_header(ostart, oc)} +{range_header(nstart, nc)} @@"
                + (heading if first else "")
            )
            first = False
            for e in piece:
                out.extend(e)
    return "\n".join(out) + "\n" if out else ""


def line_counts(diff: str) -> tuple[int, int]:
    added = deleted = 0
    for line in diff.split("\n"):
        if line.startswith("+"):
            added += 1
        elif line.startswith("-"):
            deleted += 1
    return added, deleted


def paths_of(mods) -> list[str]:
    """The paths a commit touched, as `git log --name-only` lists them (the
    old path of deletions, the new one otherwise), sorted, at most
    HISTORY_FILES."""
    paths = {m["old_path"] if m["change_type"] == "DELETE" else m["new_path"] for m in mods or []}
    return sorted(p for p in paths if p)[:HISTORY_FILES]


def inputs_of(mods) -> dict | None:
    """`trace::Inputs` without branch and subjects, or None to skip."""
    if not mods or len(mods) > MAX_FILES:
        return None
    staged, files = [], []
    for mod in sorted(mods, key=lambda m: m["new_path"] or m["old_path"] or ""):
        status = STATUS.get(mod["change_type"])
        if status is None:
            return None
        path = mod["old_path"] if status == "deleted" else mod["new_path"]
        if not path:
            return None
        old_path = mod["old_path"] if status in ("renamed", "copied") else None
        diff = mod["diff"] or ""
        if len(diff.encode()) > MAX_DIFF_BYTES:
            return None
        binary = diff.startswith("Binary files") or (
            diff == "" and status in ("added", "modified")
        )
        hunks = "" if binary else trim_context(diff)
        staged.append({"status": status, "path": path, "old_path": old_path})
        files.append(
            {
                "path": path,
                "old_path": old_path,
                "lines": None if binary else list(line_counts(hunks)),
                "hunks": hunks,
            }
        )
    return {"staged": staged, "files": files}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("out", type=Path)
    parser.add_argument("shards", type=Path, nargs="+")
    parser.add_argument("--branch", default=None, help="branch in every input (default null)")
    parser.add_argument("--releases", action="store_true",
                        help="keep only the release bumps whose version the diff adds")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    stats = {"commits": 0, "dropped_target": 0, "dropped_mods": 0, "kept": 0}
    columns = ["repo", "hash", "date", "message", "mods"]
    with open(args.out / "inputs.jsonl", "w") as inputs_out, open(
        args.out / "meta.jsonl", "w"
    ) as meta_out:
        for shard in args.shards:
            table = pq.read_table(shard, columns=columns)
            repo_col = table.column("repo").to_pylist()
            start = 0
            while start < len(repo_col):
                end = start
                while end < len(repo_col) and repo_col[end] == repo_col[start]:
                    end += 1
                rows = table.slice(start, end - start).to_pylist()
                # Stable: commits with the same date keep the shard's order.
                rows.sort(key=lambda r: date_key(r["date"]))
                subjects = [subject_of(r["message"]) for r in rows]
                touched = [paths_of(r["mods"]) for r in rows]
                for i, row in enumerate(rows):
                    stats["commits"] += 1
                    target = subjects[i]
                    if args.releases:
                        version = release_version(target)
                        keep = version is not None and SUBJECT_MIN <= len(target) <= SUBJECT_MAX
                    else:
                        version, keep = None, keep_target(target)
                    if not keep:
                        stats["dropped_target"] += 1
                        continue
                    changes = inputs_of(row["mods"])
                    if changes is None:
                        stats["dropped_mods"] += 1
                        continue
                    if version is not None and not version_in_diff(version, changes):
                        stats["dropped_target"] += 1
                        continue
                    first = max(0, i - HISTORY_DEPTH)
                    history = subjects[first:i][::-1]
                    inputs = {"branch": args.branch, "subjects": history,
                              "history_files": touched[first:i][::-1], **changes}
                    meta = {
                        "repo": row["repo"],
                        "hash": row["hash"],
                        "target": target,
                        "type": conventional_type(target),
                        "history": len(history),
                    }
                    inputs_out.write(json.dumps(inputs, ensure_ascii=False) + "\n")
                    meta_out.write(json.dumps(meta, ensure_ascii=False) + "\n")
                    stats["kept"] += 1
                start = end
    json.dump(stats, sys.stderr)
    sys.stderr.write("\n")


if __name__ == "__main__":
    main()

"""Commit scenarios: git operations on small synthetic repositories, each
with a hand-written target message.

A scenario is data, not code. `scenarios/repos/<name>/` holds a base
repository (`files/`, and `repo.toml`, the history that introduces them);
`scenarios/cases/*.toml` hold the cases, each a list of operations from
`OPS` applied on top of a base repository. Building a case runs
smart-commit's mock backend on the result and keeps the trace's inputs, so
the cases come out in the format `select.py` writes (`inputs.jsonl` and
`meta.jsonl`): `make_examples.py` turns them into training examples, and
`smart-commit eval` and `score.py` evaluate them, unchanged.

Everything is deterministic: fixed author, dates and git config, sorted
globs, seeded binary content. Building twice gives the same bytes.
"""

from __future__ import annotations

import json
import os
import random
import re
import shutil
import subprocess
import tempfile
import tomllib
import unicodedata
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).parent / "scenarios"
SPLITS = ("train", "eval")
# What the trace inputs do not record (the state of a merge or revert, file
# modes): a target naming it cannot be learnt from the inputs, so these
# cases measure only and never train.
EVAL_ONLY_TAGS = {"git-state", "file-mode"}
VAR = re.compile(r"\{\{([a-z_][a-z0-9_]*)\}\}")
COMMIT_REF = re.compile(r"\{\{commit:([^}]+)\}\}")
CONVENTIONAL = re.compile(r"^([a-z]+)(?:\([^()]*\))?!?: ")
BASE_DATE = 1767268800  # 2026-01-01T12:00:00Z


class ScenarioError(Exception):
    pass


@dataclass(frozen=True)
class Repo:
    name: str
    convention: str
    commits: list[dict]
    files: Path


@dataclass(frozen=True)
class Case:
    id: str
    title: str
    repo: str
    level: int
    split: str
    target: str
    ops: list[dict]
    expect: list[str] = field(default_factory=list)
    tags: list[str] = field(default_factory=list)
    stage: str = "all"
    history: bool = True
    outcome: str = "message"
    issue: str = ""


# ------------------------------------------------------------------ loading


def load_repos(root: Path = ROOT) -> dict[str, Repo]:
    repos = {}
    for spec in sorted((root / "repos").glob("*/repo.toml")):
        data = tomllib.loads(spec.read_text())
        name = spec.parent.name
        if data.get("convention") not in ("free", "conventional", "gitmoji"):
            raise ScenarioError(f"{spec}: convention must be free, conventional or gitmoji")
        repos[name] = Repo(name, data["convention"], data.get("commit", []), spec.parent / "files")
    return repos


def substitute(value, variables: dict[str, str]):
    """Replaces `{{name}}` in every string of `value` (nested lists and
    tables too); `{{commit:...}}` is left for `apply`, which resolves it."""
    if isinstance(value, str):
        def lookup(match: re.Match) -> str:
            if match.group(1) not in variables:
                raise ScenarioError(f"unknown variable {{{{{match.group(1)}}}}}")
            return str(variables[match.group(1)])
        return VAR.sub(lookup, value)
    if isinstance(value, list):
        return [substitute(v, variables) for v in value]
    if isinstance(value, dict):
        return {k: substitute(v, variables) for k, v in value.items()}
    return value


def expand(raw: dict, defaults: dict) -> list[Case]:
    """One case, or one per variant: a variant is a set of variables, and
    may override the split, the target and the expected words."""
    base = {**defaults, **raw}
    variants = base.pop("variants", None) or [{}]
    cases = []
    for n, variant in enumerate(variants, 1):
        variables = variant.get("vars", {})
        fields = {**base, **{k: v for k, v in variant.items() if k != "vars"}}
        if len(variants) > 1:
            fields["id"] = f"{base['id']}/{variant.get('name', n)}"
        fields.pop("name", None)
        if variables:
            fields = substitute(fields, variables)
        try:
            cases.append(Case(**fields))
        except TypeError as e:
            raise ScenarioError(f"case {fields.get('id')}: {e}") from None
    return cases


def load_cases(root: Path = ROOT, repos: dict[str, Repo] | None = None) -> list[Case]:
    repos = load_repos(root) if repos is None else repos
    cases: list[Case] = []
    for path in sorted((root / "cases").glob("*.toml")):
        data = tomllib.loads(path.read_text())
        defaults = {k: data[k] for k in ("level", "repo", "split") if k in data}
        for raw in data.get("case", []):
            cases += expand(raw, defaults)
    validate(cases, repos)
    return cases


def validate(cases: list[Case], repos: dict[str, Repo]) -> None:
    seen = set()
    for case in cases:
        where = f"case {case.id}"
        if case.id in seen:
            raise ScenarioError(f"{where}: duplicate id")
        seen.add(case.id)
        if case.repo not in repos:
            raise ScenarioError(f"{where}: unknown repo {case.repo}")
        if case.split not in SPLITS:
            raise ScenarioError(f"{where}: split must be one of {SPLITS}")
        if case.split == "train" and EVAL_ONLY_TAGS & set(case.tags):
            raise ScenarioError(f"{where}: tags {sorted(EVAL_ONLY_TAGS & set(case.tags))} are eval only")
        if case.outcome not in ("message", "error"):
            raise ScenarioError(f"{where}: outcome must be message or error")
        if case.outcome == "message" and not case.target:
            raise ScenarioError(f"{where}: no target")
        if case.stage not in ("all", "none"):
            raise ScenarioError(f"{where}: stage must be all or none")
        for op in case.ops:
            if op.get("op") not in OPS:
                raise ScenarioError(f"{where}: unknown op {op.get('op')!r}")
        for pattern in case.expect:
            re.compile(pattern)


# ---------------------------------------------------------------- git


class Git:
    """git in `cwd`, isolated from the machine's configuration, with a
    fixed identity and a clock that advances one minute per commit."""

    def __init__(self, cwd: Path) -> None:
        self.cwd = cwd
        self.ticks = 0

    def env(self) -> dict[str, str]:
        date = f"@{BASE_DATE + 60 * self.ticks} +0000"
        return {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(self.cwd),
            "LANG": "C.UTF-8",
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Scenario", "GIT_AUTHOR_EMAIL": "scenario@example.com",
            "GIT_COMMITTER_NAME": "Scenario", "GIT_COMMITTER_EMAIL": "scenario@example.com",
            "GIT_AUTHOR_DATE": date, "GIT_COMMITTER_DATE": date,
        }

    def __call__(self, *args: str, check: bool = True) -> str:
        self.ticks += 1
        result = subprocess.run(
            ["git", "-c", "core.autocrlf=false", "-c", "protocol.file.allow=always", *args],
            cwd=self.cwd, env=self.env(), capture_output=True, text=True,
        )
        if check and result.returncode != 0:
            raise ScenarioError(f"git {' '.join(args)}: {result.stderr.strip()}")
        return result.stdout


def build_repo(repo: Repo, dest: Path, history: bool = True) -> Git:
    """The base repository in `dest`. Without history, its final files are
    there, untracked, in a repository with no commit."""
    dest.mkdir(parents=True)
    git = Git(dest)
    git("init", "-q", "-b", "main")
    if not history:
        shutil.copytree(repo.files, dest, dirs_exist_ok=True, symlinks=True)
        for commit in repo.commits:
            for op in commit.get("ops", []):
                run_op(op, dest, git)
        return git
    introduced = set()
    for commit in repo.commits:
        for path in commit.get("paths", []):
            source = repo.files / path
            if not source.is_file():
                raise ScenarioError(f"repo {repo.name}: no file {path}")
            target = dest / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
            introduced.add(path)
        for op in commit.get("ops", []):
            run_op(op, dest, git)
        git("add", "-A")
        git("commit", "-q", "-m", commit["message"])
    missing = {str(p.relative_to(repo.files)) for p in repo.files.rglob("*") if p.is_file()} - introduced
    if missing:
        raise ScenarioError(f"repo {repo.name}: files no commit introduces: {sorted(missing)}")
    return git


# ---------------------------------------------------------------- operations


def inside(root: Path, path: str) -> Path:
    resolved = (root / path).resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise ScenarioError(f"path outside the repository: {path}")
    return root / path


def files_of(root: Path, op: dict) -> list[Path]:
    """The files an operation names: `path`, or `paths`, globs allowed."""
    patterns = op.get("paths") or [op["path"]]
    found: list[Path] = []
    for pattern in patterns:
        matches = sorted(p for p in root.glob(pattern) if p.is_file() and ".git" not in p.parts)
        if not matches:
            raise ScenarioError(f"no file matches {pattern}")
        found += matches
    return found


def read(path: Path) -> str:
    return path.read_bytes().decode()


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode())


def text_of(op: dict) -> str:
    """The operation's text, or `repeat` copies of it with `{{i}}` counting
    from 0, for large files."""
    if "repeat" not in op:
        return op["text"]
    return op.get("join", "").join(op["text"].replace("{{i}}", str(i)) for i in range(op["repeat"]))


def op_write(root: Path, git: Git, op: dict) -> None:
    write(inside(root, op["path"]), text_of(op))


def op_append(root: Path, git: Git, op: dict) -> None:
    for path in files_of(root, op):
        write(path, read(path) + text_of(op))


def op_prepend(root: Path, git: Git, op: dict) -> None:
    for path in files_of(root, op):
        write(path, text_of(op) + read(path))


def op_replace(root: Path, git: Git, op: dict) -> None:
    """Literal replacement; `old` must occur exactly once unless `all`."""
    for path in files_of(root, op):
        text = read(path)
        count = text.count(op["old"])
        if count == 0 or (count > 1 and not op.get("all")):
            raise ScenarioError(f"{path.name}: {count} occurrences of {op['old']!r}")
        write(path, text.replace(op["old"], op["new"]))


def op_regex(root: Path, git: Git, op: dict) -> None:
    pattern = re.compile(op["pattern"], re.M)
    for path in files_of(root, op):
        text, n = pattern.subn(op["repl"], read(path))
        if n == 0 and not op.get("optional"):
            raise ScenarioError(f"{path.name}: no match for {op['pattern']!r}")
        write(path, text)


def op_remove(root: Path, git: Git, op: dict) -> None:
    path = inside(root, op["path"])
    shutil.rmtree(path) if path.is_dir() else path.unlink()


def op_move(root: Path, git: Git, op: dict) -> None:
    target = inside(root, op["to"])
    target.parent.mkdir(parents=True, exist_ok=True)
    inside(root, op["from"]).rename(target)


def op_chmod(root: Path, git: Git, op: dict) -> None:
    inside(root, op["path"]).chmod(int(op["mode"], 8))


def op_symlink(root: Path, git: Git, op: dict) -> None:
    inside(root, op["path"]).symlink_to(op["target"])


def op_binary(root: Path, git: Git, op: dict) -> None:
    data = random.Random(op.get("seed", 0)).randbytes(op["size"])
    path = inside(root, op["path"])
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def op_git(root: Path, git: Git, op: dict) -> None:
    """A git command; `{{commit:subject}}` names the commit with that subject."""
    def resolve(match: re.Match) -> str:
        found = git("log", "--all", "--format=%H", "--fixed-strings", "--grep", match.group(1)).split()
        if len(found) != 1:
            raise ScenarioError(f"{len(found)} commits match {match.group(1)!r}")
        return found[0]
    git(*[COMMIT_REF.sub(resolve, arg) for arg in op["args"]], check=op.get("check", True))


OPS = {
    "write": op_write, "append": op_append, "prepend": op_prepend, "replace": op_replace,
    "regex": op_regex, "remove": op_remove, "move": op_move, "chmod": op_chmod,
    "symlink": op_symlink, "binary": op_binary, "git": op_git,
}


def run_op(op: dict, root: Path, git: Git) -> None:
    OPS[op["op"]](root, git, op)


def apply(case: Case, repo: Repo, dest: Path) -> Git:
    """Builds `case`'s repository in `dest`, with its changes staged."""
    git = build_repo(repo, dest, case.history)
    for n, op in enumerate(case.ops, 1):
        try:
            run_op(op, dest, git)
        except (ScenarioError, OSError, KeyError) as e:
            raise ScenarioError(f"case {case.id}, op {n} ({op['op']}): {e}") from None
    if case.stage == "all":
        git("add", "-A")
    return git


# ---------------------------------------------------------------- capture


def capture(workdir: Path, binary: str) -> tuple[dict | None, str]:
    """The trace inputs smart-commit records for `workdir`'s staged changes,
    from its mock backend, with an empty configuration; or None and the
    error when it refuses to generate."""
    with tempfile.TemporaryDirectory() as home:
        trace = Path(home) / "trace.json"
        env = {**Git(workdir).env(), "HOME": home, "XDG_CONFIG_HOME": f"{home}/config",
               "XDG_CACHE_HOME": f"{home}/cache", "XDG_DATA_HOME": f"{home}/data"}
        result = subprocess.run(
            [binary, "-C", str(workdir), "--backend", "mock", "--mock-response", "mock",
             "--trace", str(trace)],
            env=env, capture_output=True, text=True,
        )
        if result.returncode != 0 or not trace.exists():
            return None, (result.stderr or result.stdout).strip()
        return json.loads(trace.read_text())["inputs"], ""


def meta_of(case: Case, repo: Repo, inputs: dict) -> dict:
    """The `meta.jsonl` line: what `score.py` and `make_examples.py` read,
    plus what the scenarios add (level, tags, split, expected words)."""
    match = CONVENTIONAL.match(case.target)
    convention = "conventional" if repo.convention == "conventional" and case.history else "free"
    # git's own messages (Merge ..., Revert "...") count as types merge and revert.
    kind = match.group(1) if match else plain(case.target.split(" ", 1)[0]) or None
    return {
        "repo": f"scenario/{case.repo}", "hash": case.id, "target": case.target,
        "type": kind, "history": len(inputs["subjects"]),
        "set": convention, "title": case.title, "level": case.level, "split": case.split,
        "tags": case.tags, "expect": case.expect,
    }


def plain(text: str) -> str:
    """Lowercase without accents, which `expect` patterns are matched on."""
    text = unicodedata.normalize("NFKD", text)
    return "".join(c for c in text if not unicodedata.combining(c)).lower()


def expected(subject: str, patterns: list[str]) -> bool | None:
    """Whether `subject` has what a right message must say: every pattern
    matches. None when the case lists none."""
    if not patterns:
        return None
    return all(re.search(p, plain(subject)) for p in patterns)


def select(cases: list[Case], split: str = "all", only: str = "") -> list[Case]:
    return [c for c in cases
            if (split == "all" or c.split == split) and c.id.startswith(only)]



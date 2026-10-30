"""An audit set of the acceptable commit types, written by two blind annotators (H17).

`type_match` takes the repository's own type as the only right one, but the same change can often be a `fix` or a
`feat`, a `chore` or a `build`. An audit set says, for each of a sample of commits, which types a reasonable
maintainer would accept, so that a model is judged by whether its type is among them.

- `build` draws N Conventional commits of a set (seeded) and writes, for each of two annotators, batches of cases to
  read: the repository's recent subjects, the files with their status and the diff, never the real subject or type.
  The key (which case is which commit) goes in `sample.json`, apart.
- `merge` reads the annotations the annotators wrote (`A-1.json`..., `B-1.json`...: a list of `id`, `acceptable`,
  `best`, `ambiguous`, `reason`), checks them and writes `audit.json`.
- `report` shows how far the two agree, whether each one's set holds the real type, and how models score: `strict`
  (the real type), `lenient` (a type either annotator accepts) and `agreed` (one both accept; where they share none,
  the case counts only for `lenient`).

    python -I audit_types.py build SET_DIR OUT_DIR [--n 100] [--seed 17] [--batches 4]
    python -I audit_types.py merge OUT_DIR
    python -I audit_types.py report OUT_DIR/audit.json [NAME=TRACES.jsonl ...] [--set SET_DIR]

Standard library only.
"""

import collections
import json
import random
import sys
from pathlib import Path

TYPES = ["feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert"]
HISTORY_SHOWN = 12
FILE_CHARS = 1200
DIFF_CHARS = 5000
ANNOTATORS = ("A", "B")


def fold(kind: str | None) -> str | None:
    """`tests` is `test`; anything else is as it is, lowercase."""
    kind = (kind or "").strip().lower()
    return "test" if kind == "tests" else (kind or None)


def render_case(cid: str, inputs: dict) -> str:
    """One case as the annotator reads it: no repository name, no hash, no real subject or type."""
    lines = [f"## Case {cid}", "", "Recent commit subjects of this repository (newest first):"]
    lines += [f"- {s}" for s in (inputs.get("subjects") or [])[:HISTORY_SHOWN]] or ["- (none)"]
    lines += ["", "Changes in this commit (status, path, lines added/removed):"]
    status = {s["path"]: s["status"] for s in inputs.get("staged") or []}
    for f in inputs["files"]:
        counts = f.get("lines")
        shown = f"+{counts[0]} -{counts[1]}" if counts else "binary"
        lines.append(f"- {status.get(f['path'], 'modified')} {f['path']} ({shown})")
    lines += ["", "Diff (cut where it is long):", "```diff"]
    used = 0
    for f in inputs["files"]:
        hunks = f.get("hunks") or ""
        if not hunks:
            continue
        part = hunks[:FILE_CHARS] + ("\n... (cut)" if len(hunks) > FILE_CHARS else "")
        if used + len(part) > DIFF_CHARS:
            lines.append(f"... ({f['path']} and later files not shown)")
            break
        used += len(part)
        lines += [f"--- {f['path']}", part]
    lines.append("```")
    return "\n".join(lines)


def conventional_lines(set_dir: Path) -> list[int]:
    out = []
    for index, line in enumerate(open(set_dir / "meta.jsonl")):
        meta = json.loads(line)
        if meta.get("set") == "conventional" and meta.get("type"):
            out.append(index)
    return out


def sample_cases(pool: list[int], n: int, seed: int) -> list[int]:
    """`n` lines of the pool, sorted, the same for the same seed."""
    return sorted(random.Random(seed).sample(pool, min(n, len(pool))))


def batches_for(ids: list[str], annotator: str, seed: int, count: int) -> list[list[str]]:
    """The ids in an order of their own for each annotator, in `count` batches of nearly the same size."""
    order = list(ids)
    random.Random(f"{seed}-{annotator}").shuffle(order)
    return [order[i::count] for i in range(count)]


def build(set_dir: Path, out_dir: Path, n: int, seed: int, count: int) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    metas = [json.loads(line) for line in open(set_dir / "meta.jsonl")]
    inputs = [json.loads(line) for line in open(set_dir / "inputs.jsonl")]
    lines = sample_cases(conventional_lines(set_dir), n, seed)
    ids = [f"c{k:03d}" for k in random.Random(seed + 1).sample(range(1, 1000), len(lines))]
    key = [
        {"id": cid, "line": line, "repo": metas[line]["repo"], "hash": metas[line]["hash"],
         "real_subject": metas[line]["target"], "real_type": fold(metas[line]["type"])}
        for cid, line in zip(ids, lines)
    ]
    (out_dir / "sample.json").write_text(json.dumps(key, indent=1))
    by_id = {k["id"]: inputs[k["line"]] for k in key}
    for annotator in ANNOTATORS:
        for number, batch in enumerate(batches_for(ids, annotator, seed, count), 1):
            text = "\n\n".join(render_case(cid, by_id[cid]) for cid in batch)
            (out_dir / f"{annotator}-{number}.md").write_text(text + "\n")
            (out_dir / f"{annotator}-{number}.ids").write_text("\n".join(batch) + "\n")


def check(annotation: dict, valid_ids: set[str]) -> list[str]:
    """What is wrong with one annotation (nothing: empty list)."""
    problems = []
    if annotation.get("id") not in valid_ids:
        problems.append(f"unknown id {annotation.get('id')!r}")
    accepted = [fold(t) for t in annotation.get("acceptable") or []]
    if not accepted or any(t not in TYPES for t in accepted):
        problems.append(f"acceptable {annotation.get('acceptable')!r} is empty or has an unknown type")
    best = fold(annotation.get("best"))
    if best not in accepted:
        problems.append(f"best {annotation.get('best')!r} is not in acceptable")
    return problems


def merge(out_dir: Path) -> None:
    key = json.loads((out_dir / "sample.json").read_text())
    valid = {k["id"] for k in key}
    merged = {k["id"]: {**k} for k in key}
    for annotator in ANNOTATORS:
        seen: set[str] = set()
        for path in sorted(out_dir.glob(f"{annotator}-*.json")):
            for annotation in json.loads(path.read_text()):
                problems = check(annotation, valid)
                if problems:
                    sys.exit(f"{path.name}: {annotation.get('id')}: {'; '.join(problems)}")
                cid = annotation["id"]
                if cid in seen:
                    sys.exit(f"{annotator}: {cid} annotated twice")
                seen.add(cid)
                merged[cid][annotator] = {
                    "acceptable": sorted({fold(t) for t in annotation["acceptable"]}, key=TYPES.index),
                    "best": fold(annotation["best"]),
                    "ambiguous": bool(annotation.get("ambiguous")),
                    "reason": str(annotation.get("reason", ""))[:240],
                }
        missing = valid - seen
        if missing:
            sys.exit(f"{annotator}: {len(missing)} cases not annotated, e.g. {sorted(missing)[:3]}")
    (out_dir / "audit.json").write_text(json.dumps(list(merged.values()), indent=1))
    print(f"{len(merged)} cases merged into {out_dir / 'audit.json'}")


def jaccard(a: set, b: set) -> float:
    return len(a & b) / len(a | b) if a | b else 1.0


def cohen_kappa(x: list[str], y: list[str]) -> float:
    """Cohen's kappa of two labelings of the same cases (1 is the same, 0 no better than chance)."""
    n = len(x)
    observed = sum(a == b for a, b in zip(x, y)) / n
    cx, cy = collections.Counter(x), collections.Counter(y)
    expected = sum(cx[k] * cy[k] for k in set(cx) | set(cy)) / (n * n)
    return 1.0 if expected == 1 else (observed - expected) / (1 - expected)


def sets_of(case: dict) -> tuple[set, set]:
    return set(case["A"]["acceptable"]), set(case["B"]["acceptable"])


def judge(kind: str | None, case: dict) -> dict:
    """How one type fares on one case: the real one, one either annotator accepts, one both accept."""
    a, b = sets_of(case)
    return {
        "strict": kind == case["real_type"],
        "lenient": kind in (a | b),
        "agreed": kind in (a & b) if a & b else None,
    }


def agreement(cases: list[dict]) -> dict:
    n = len(cases)
    a_sets, b_sets = zip(*(sets_of(c) for c in cases))
    return {
        "n": n,
        "same_set": sum(x == y for x, y in zip(a_sets, b_sets)) / n,
        "mean_jaccard": sum(jaccard(x, y) for x, y in zip(a_sets, b_sets)) / n,
        "share_a_type": sum(bool(x & y) for x, y in zip(a_sets, b_sets)) / n,
        "same_best": sum(c["A"]["best"] == c["B"]["best"] for c in cases) / n,
        "kappa_best": cohen_kappa([c["A"]["best"] for c in cases], [c["B"]["best"] for c in cases]),
        "size_a": sum(map(len, a_sets)) / n,
        "size_b": sum(map(len, b_sets)) / n,
        "ambiguous_a": sum(c["A"]["ambiguous"] for c in cases) / n,
        "ambiguous_b": sum(c["B"]["ambiguous"] for c in cases) / n,
        "real_in_a": sum(c["real_type"] in x for c, x in zip(cases, a_sets)) / n,
        "real_in_b": sum(c["real_type"] in y for c, y in zip(cases, b_sets)) / n,
        "best_is_real_a": sum(c["A"]["best"] == c["real_type"] for c in cases) / n,
        "best_is_real_b": sum(c["B"]["best"] == c["real_type"] for c in cases) / n,
    }


def model_types(traces: Path, lines: list[int]) -> list[str | None]:
    wanted = set(lines)
    got = {}
    for index, line in enumerate(open(traces)):
        if index in wanted:
            subject = json.loads(line)["generation"]["message"]["subject"]
            head = subject.split(":", 1)[0].split("(", 1)[0].strip().rstrip("!") if ":" in subject else ""
            got[index] = fold(head) if head and " " not in head else None
    return [got[i] for i in lines]


def score_model(types: list[str | None], cases: list[dict]) -> dict:
    n = len(cases)
    judged = [judge(t, c) for t, c in zip(types, cases)]
    agreed = [j["agreed"] for j in judged if j["agreed"] is not None]
    return {
        "strict": sum(j["strict"] for j in judged) / n,
        "lenient": sum(j["lenient"] for j in judged) / n,
        "agreed": sum(agreed) / len(agreed) if agreed else 0.0,
        "agreed_n": len(agreed),
    }


def report(audit: Path, models: list[str], set_dir: Path | None) -> None:
    cases = json.loads(audit.read_text())
    a = agreement(cases)
    print(f"{a['n']} cases, two blind annotators")
    print(f"  accepted types per case: A {a['size_a']:.2f}, B {a['size_b']:.2f}; marked ambiguous: A {a['ambiguous_a']:.2f}, "
          f"B {a['ambiguous_b']:.2f}")
    print(f"  the same set {a['same_set']:.2f}, mean Jaccard {a['mean_jaccard']:.2f}, share a type {a['share_a_type']:.2f}; "
          f"same best {a['same_best']:.2f} (kappa {a['kappa_best']:.2f})")
    print(f"  the real type is in the set: A {a['real_in_a']:.2f}, B {a['real_in_b']:.2f}; is the best: A "
          f"{a['best_is_real_a']:.2f}, B {a['best_is_real_b']:.2f}")
    real = collections.Counter(c["real_type"] for c in cases)
    print("  real types:", ", ".join(f"{k} {v}" for k, v in real.most_common()))
    if models and set_dir:
        lines = [c["line"] for c in cases]
        for spec in models:
            name, path = spec.split("=", 1)
            s = score_model(model_types(Path(path), lines), cases)
            print(f"  {name:12s} strict {s['strict']:.3f}  lenient {s['lenient']:.3f}  agreed {s['agreed']:.3f} "
                  f"(of the {s['agreed_n']} cases where both annotators share a type)")


def main() -> None:
    args = sys.argv[1:]
    flag = lambda name, default: type(default)(args[args.index(name) + 1]) if name in args else default  # noqa: E731
    if args[:1] == ["build"]:
        build(Path(args[1]), Path(args[2]), flag("--n", 100), flag("--seed", 17), flag("--batches", 4))
    elif args[:1] == ["merge"]:
        merge(Path(args[1]))
    elif args[:1] == ["report"]:
        models = [a for a in args[2:] if "=" in a]
        set_dir = Path(args[args.index("--set") + 1]) if "--set" in args else None
        report(Path(args[1]), models, set_dir)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()

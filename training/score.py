"""Scores `smart-commit eval` traces against the real subjects.

    python -I score.py EVAL_DIR TRACES.jsonl [--json]

Per set (conventional, free) and overall:
- first_try: the first reply passed the sanitizer
- fallback: every attempt failed; the message came from the file list
- copied: the subject is one of the history subjects verbatim
- first_copied: the first reply repeated a history subject (smart-commit
  rejects that and retries, so copied alone hides it)
- type_match: same Conventional type as the real subject (conventional set),
  with spellings of one type folded (`tests` is `test`)
- type_macro_f1: the F1 of each real type, averaged over the types, with
  each type's precision, recall and F1 in type_scores: type_match is
  dominated by `fix`, the most frequent type
- scope_known: no scope, or one already used in the history (conventional set)
- scope_fits: the scope was used by history commits touching a directory
  the staged files are in, or there is no scope and no history scope does
  (conventional set; needs `history_files` in the inputs). scope_fits_real
  is the same for the real subject
- word_f1: word overlap with the real subject, a cheap proxy for content
- exact: identical to the real subject
- unsupported: names a specific (a version or number, an identifier in
  camelCase, snake_case or backticks) found neither in the diff, the paths
  nor the history: likely invented
- ungrounded: no content word of the description is in the diff or the
  paths: likely vague ("fix typo", "update code")
- unsupported_real / ungrounded_real: the same for the real subject, the
  level a human reaches with the same diff (often more than the prompt
  shows, since the diff is cut to fit)

Metrics of the conventional set are averaged over that set in "all" too.
"""

import argparse
import collections
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from build_inputs import TYPE_ALIASES, conventional_type  # noqa: E402

SCOPE = re.compile(r"^[a-z]+\(([^()]+)\)")
WORD = re.compile(r"[a-z0-9]+")
HEADER = re.compile(r"^(?:[a-z]+(?:\([^()]*\))?!?:\s*|\W+\s*)")
SPECIFIC = re.compile(
    r"`([^`]+)`"                              # `anything`
    r"|\b(v?\d+(?:\.\d+)+[\w.-]*)"          # versions: 1.2, v5.5.0-beta.6
    r"|\b(\d{2,})\b"                          # numbers of 2+ digits
    r"|\b([a-z]+[A-Z]\w*|[A-Z][a-z0-9]+[A-Z]\w*)\b"  # camelCase, PascalCase
    r"|\b(\w+_\w+)\b"                         # snake_case
)
STOP = set("""a an the and or of to in on for with from by into onto when then
than that this these those it its is are be was were as at not no only all
any some more less new old use used using add adds added fix fixes fixed
update updates updated remove removes removed make makes made change changes
changed support allow allows move moves rename renames improve improves
refactor clean cleanup minor small bug bugs issue issues code file files
test tests docs doc""".split())


def description(subject: str) -> str:
    """The subject without its Conventional header or leading emoji."""
    return HEADER.sub("", subject, count=1)


def specifics(text: str) -> list[str]:
    return [next(g for g in m.groups() if g) for m in SPECIFIC.finditer(text)]


def evidence(inputs: dict) -> str:
    parts = list(inputs["subjects"])
    for f in inputs["files"]:
        parts += [f["path"], f.get("old_path") or "", f["hunks"]]
    return "\n".join(parts).lower()


def area(path: str) -> str:
    """The directory a file is in, with a trailing slash; "" at the root."""
    return path.rsplit("/", 1)[0] + "/" if "/" in path else ""


def scope_fits(subject: str, inputs: dict) -> bool | None:
    if "history_files" not in inputs:
        return None
    staged = {area(f["path"]) for f in inputs["files"]}
    covered = collections.defaultdict(set)
    for previous, files in zip(inputs["subjects"], inputs["history_files"]):
        if m := SCOPE.match(previous):
            covered[m.group(1)] |= {area(p) for p in files}
    m = SCOPE.match(subject)
    if m is None:
        return not any(areas & staged for areas in covered.values())
    return bool(covered.get(m.group(1), set()) & staged)


def unsupported(subject: str, known: str) -> bool:
    return any(s.lower() not in known for s in specifics(description(subject)))


def ungrounded(subject: str, diff: str) -> bool:
    content = [w for w in words(description(subject)) if len(w) >= 3 and w not in STOP]
    return not any(w in diff for w in content)


def words(text: str) -> list[str]:
    return WORD.findall(text.lower())


def f1(a: str, b: str) -> float:
    x, y = collections.Counter(words(a)), collections.Counter(words(b))
    common = sum((x & y).values())
    if not common:
        return 0.0
    p, r = common / sum(x.values()), common / sum(y.values())
    return 2 * p * r / (p + r)


def conv_type(subject: str) -> str | None:
    return conventional_type(subject)


def per_type(pairs: collections.Counter) -> tuple[dict, float]:
    """Precision, recall and F1 of each real type, from (real, generated)
    counts, and their mean over the real types (macro F1): accuracy alone is
    dominated by `fix`, the most frequent type."""
    real, generated, hits = collections.Counter(), collections.Counter(), collections.Counter()
    for (r, g), n in pairs.items():
        real[r] += n
        generated[g] += n
        if r == g:
            hits[r] += n
    scores = {}
    for t in sorted(real, key=lambda t: (-real[t], t)):
        p = hits[t] / generated[t] if generated[t] else 0.0
        r = hits[t] / real[t]
        f = 2 * p * r / (p + r) if p + r else 0.0
        scores[t] = {"n": real[t], "precision": round(p, 3), "recall": round(r, 3), "f1": round(f, 3)}
    macro = sum(s["f1"] for s in scores.values()) / len(scores) if scores else 0.0
    return scores, round(macro, 3)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("eval_dir", type=Path)
    parser.add_argument("traces", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    meta = [json.loads(l) for l in (args.eval_dir / "meta.jsonl").read_text().splitlines()]
    traces = [json.loads(l) for l in args.traces.read_text().splitlines()]
    assert len(meta) == len(traces), (len(meta), len(traces))

    sums: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    counts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    types: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    for m, t in zip(meta, traces):
        g = t["generation"]
        subject = g["message"]["subject"]
        history = t["inputs"]["subjects"]
        known = evidence(t["inputs"])
        diff = evidence({**t["inputs"], "subjects": []})
        row = {
            "unsupported": unsupported(subject, known),
            "unsupported_real": unsupported(m["target"], known),
            "ungrounded": ungrounded(subject, diff),
            "ungrounded_real": ungrounded(m["target"], diff),
            "first_try": g["attempts"][0]["rejected"] is None,
            "fallback": g["source"] == "fallback",
            "copied": subject in history,
            "first_copied": "copied_history" in (g["attempts"][0]["rejected"] or {}),
            "word_f1": f1(subject, m["target"]),
            "exact": subject == m["target"],
        }
        if m["set"] == "conventional":
            real_type = TYPE_ALIASES.get(m["type"], m["type"])
            row["type_match"] = conv_type(subject) == real_type
            scope = SCOPE.match(subject)
            known = {s.group(1) for h in history if (s := SCOPE.match(h))}
            row["scope_known"] = scope is None or scope.group(1) in known
            fits = scope_fits(subject, t["inputs"])
            if fits is not None:
                row["scope_fits"] = fits
                row["scope_fits_real"] = scope_fits(m["target"], t["inputs"])
        for key in (m["set"], "all"):
            sums[key]["n"] += 1
            for k, v in row.items():
                sums[key][k] += float(v)
                counts[key][k] += 1
            if m["set"] == "conventional":
                types[key][(real_type, conv_type(subject))] += 1

    report = {}
    for key, c in sums.items():
        n = c.pop("n")
        report[key] = {"n": int(n), **{k: round(v / counts[key][k], 3) for k, v in sorted(c.items())}}
        if types[key]:
            scores, macro = per_type(types[key])
            report[key]["type_macro_f1"] = macro
            report[key]["types (real->generated)"] = {
                f"{r}->{g}": n for (r, g), n in types[key].most_common()
            }
            report[key]["type_scores"] = scores
    if args.json:
        print(json.dumps(report, indent=2))
        return
    for key, r in report.items():
        print(f"[{key}] n={r.pop('n')}")
        for k, v in r.items():
            print(f"  {k:<14} {v}")


if __name__ == "__main__":
    main()

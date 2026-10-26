"""Runs models on evaluation sets with `smart-commit eval` and compares them
with a baseline using `sc-training score`: the metrics of each, and for each
metric how many cases a model gains and loses against the baseline (paired,
with McNemar's p).

    python -I compare_models.py OUT SMART_COMMIT SC_TRAINING --sets DIR... \\
        --baseline NAME=GGUF CANDIDATE=GGUF ...

The traces of a model on a set are kept in OUT/SET/NAME.jsonl and not made
again, so adding a candidate costs only that candidate. `--rules` and `--diff`
are those the models were trained on (v0.1: `full` and `plain`). The summary
goes to OUT/summary.json.
"""

import argparse
import collections
import json
import os
import re
import statistics
import subprocess
import tempfile
from pathlib import Path

# Metrics where a case flagged is a case right, and where it is a case wrong.
GOOD = ["first_try", "type_match", "scope_known", "scope_fits", "exact", "expected"]
BAD = ["ungrounded", "unsupported", "fallback", "first_copied", "copied"]
SHOWN = ["type_match", "type_macro_f1", "scope_known", "scoped", "ungrounded", "first_copied", "first_try",
         "fallback", "word_f1", "exact", "expected"]


def columns(rows: list[dict]) -> list[str]:
    """The metrics of SHOWN that at least one model has: `expected` exists only for
    the scenarios, and a column of dashes says nothing."""
    return [m for m in SHOWN if any(m in scores for scores in rows)]


def better(metric: str, gained: int, lost: int) -> int:
    """Cases a model improves over the baseline, net: a flag gained is better
    for `GOOD` metrics and worse for `BAD` ones."""
    return gained - lost if metric in GOOD else lost - gained if metric in BAD else 0


def mean_subject_length(traces: Path) -> float:
    lengths = [len(json.loads(line)["generation"]["message"]["subject"])
               for line in traces.read_text().splitlines()]
    return round(statistics.mean(lengths), 1) if lengths else 0.0


HEADER = re.compile(r"^(\w+)(\([^()]*\))?!?:")


def habits(directory: Path, traces: Path | None = None) -> dict:
    """Per set, the share of subjects with a scope and the count of each type,
    of the model's subjects (`traces`) or of the real ones. A model that stops
    writing scopes has a `scope_known` of 1.0, and one that writes `chore` for
    everything a `first_try` of 1.0: only the habits show it."""
    meta = [json.loads(line) for line in (directory / "meta.jsonl").read_text().splitlines()]
    if traces:
        subjects = [json.loads(line)["generation"]["message"]["subject"] for line in traces.read_text().splitlines()]
    else:
        subjects = [m["target"] for m in meta]
    found: dict = {}
    for group in ("conventional", "free"):
        mine = [s for m, s in zip(meta, subjects) if m["set"] == group]
        headers = [HEADER.match(s) for s in mine]
        types = collections.Counter(("test" if h[1] == "tests" else h[1]) if h else "-" for h in headers)
        found[group] = {"n": len(mine), "scoped": round(sum(bool(h and h[2]) for h in headers) / max(len(mine), 1), 3),
                        "types": dict(types.most_common())}
    return found


def line_count(path: Path) -> int:
    return len(path.read_text().splitlines()) if path.exists() else 0


def run_eval(binary: str, gguf: str, inputs: Path, out: Path, rules: str, diff: str, device: str) -> bool:
    """Writes the traces of `gguf` on `inputs` to `out` unless they are all
    there; whether it ran."""
    if line_count(out) == line_count(inputs) > 0:
        return False
    out.parent.mkdir(parents=True, exist_ok=True)
    # smart-commit takes an absolute path only, and the script may run from anywhere.
    gguf = str(Path(gguf).expanduser().resolve())
    with tempfile.TemporaryDirectory() as config_home:
        env = {**os.environ, "XDG_CONFIG_HOME": config_home, "SMART_COMMIT_GGUF": gguf}
        done = subprocess.run([binary, "eval", str(inputs), "--device", device, "--rules", rules, "--diff", diff],
                              env=env, capture_output=True, text=True)
    if done.returncode:
        out.unlink(missing_ok=True)
        raise SystemExit(f"{binary} eval of {gguf} failed ({done.returncode}):\n{done.stderr[-1500:]}")
    out.write_text(done.stdout)
    return True


def score(sct: str, directory: Path, traces: Path, against: Path | None = None) -> dict:
    command = [sct, "score", str(directory), str(traces), "--json"]
    if against:
        command += ["--against", str(against)]
    return json.loads(subprocess.run(command, capture_output=True, text=True, check=True).stdout)


def table(rows: list[list[str]]) -> str:
    widths = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    return "\n".join("  ".join(c.ljust(w) if i == 0 else c.rjust(w) for i, (c, w) in enumerate(zip(r, widths)))
                     for r in rows)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("out", type=Path)
    parser.add_argument("binary", help="smart-commit")
    parser.add_argument("sct", help="sc-training")
    parser.add_argument("--sets", type=Path, nargs="+", required=True, metavar="DIR")
    parser.add_argument("--baseline", required=True, metavar="NAME=GGUF")
    parser.add_argument("candidates", nargs="*", metavar="NAME=GGUF")
    parser.add_argument("--rules", choices=["full", "omitted"], default="full")
    parser.add_argument("--diff", choices=["plain", "annotated"], default="plain")
    parser.add_argument("--device", default="vulkan")
    args = parser.parse_args()
    models = dict(m.split("=", 1) for m in [args.baseline, *args.candidates])
    base = args.baseline.split("=", 1)[0]
    summary: dict = {}
    for directory in args.sets:
        traces = {name: args.out / directory.name / f"{name}.jsonl" for name in models}
        for name, gguf in models.items():
            if run_eval(args.binary, gguf, directory / "inputs.jsonl", traces[name], args.rules, args.diff, args.device):
                print(f"{directory.name}: {name} evaluated", flush=True)
        for name in models:
            scored = score(args.sct, directory, traces[name])
            paired = score(args.sct, directory, traces[name], traces[base]) if name != base else {}
            summary.setdefault(directory.name, {})[name] = {
                "scores": scored, "against_baseline": paired, "mean_subject_chars": mean_subject_length(traces[name]),
                "habits": habits(directory, traces[name])}
        summary[directory.name]["real"] = {"habits": habits(directory)}
        for group in ("conventional", "free"):
            shown = columns([{**summary[directory.name][m]["scores"].get(group, {}),
                              **({"scoped": 0} if group == "conventional" else {})} for m in models])
            rows = [["model", *shown, "chars", "net vs baseline (* p < 0.05)"]]
            for name in models:
                entry = summary[directory.name][name]
                scores = dict(entry["scores"].get(group, {}))
                if group == "conventional":
                    scores["scoped"] = entry["habits"][group]["scoped"]
                else:
                    scores.pop("scoped", None)
                paired = entry["against_baseline"].get(group, {})
                nets = " ".join(f"{m}{better(m, p['gained'], p['lost']):+d}{'*' if p['p'] < 0.05 else ''}"
                                for m, p in paired.items() if m in SHOWN and better(m, p["gained"], p["lost"]))
                rows.append([name, *[f"{scores[m]:.3f}" if m in scores else "-" for m in shown],
                             f"{entry['mean_subject_chars']}", nets or "-"])
            print(f"\n== {directory.name} / {group}\n{table(rows)}")
            if group == "conventional":
                real = summary[directory.name]["real"]["habits"][group]
                print(f"\ntypes (n={real['n']}, scoped {real['scoped']}); "
                      + "\n".join(f"  {n:12} " + " ".join(f"{t} {c}" for t, c in list(h["types"].items())[:7])
                                  for n, h in [("real", real), *[(m, summary[directory.name][m]["habits"][group]) for m in models]]))
    (args.out / "summary.json").write_text(json.dumps(summary, indent=1) + "\n")


if __name__ == "__main__":
    main()

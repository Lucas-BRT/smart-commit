"""How well a model knows when it is unsure of the commit type (H13, A).

`model.ask_type` asks the person when the probability of the type a reply starts with is low. This reads the same
probabilities, from the first token of the reply (llama-server, `top_logprobs`, the prompts of the traces
`sc-training compare` left), and reports how far they can be trusted:

- `top1`, `top2`, `top3`: the real type is the likeliest, among the two, among the three likeliest;
- `ece`: expected calibration error, over CALIBRATION_BINS equal-width bins of the confidence in the likeliest type;
- `brier`, `nll`: the squared error and the log loss of the distribution against the real type;
- `auroc`: the chance that a commit the model gets right has a higher confidence than one it gets wrong;
- `bins`: commits and share right by confidence, as `docs/measurements.md` shows them;
- `asking`: asking below a confidence, the share of commits asked and the type right if the person picks the real
  type when it is among the three likeliest.

    python -I type_confidence.py collect SET_DIR TRACES.jsonl GGUF OUT.json [--port N]
    python -I type_confidence.py report OUT.json...
    python -I type_confidence.py menus OUT.json...
    python -I type_confidence.py sets CALIBRATION.json TEST.json

`menus` compares the menu `model.ask_type` shows (up to MAX_OFFERED types) with an adaptive one that offers only
the two likeliest when the model is split between them (together at least SPLIT_MASS, apart by less than
SPLIT_GAP), on the same asks: the share asked, the lines of the menu, and the type right if the person picks the
real type when it is offered.

`sets` offers, instead of a fixed number of types, the smallest prefix of the likeliest ones that holds a mass `q`
(conformal prediction sets), asking when it has two types or more. `q` is the CONFORMAL level of the mass above
the real type on CALIBRATION, so a share `1 - alpha` of the commits has the real type in the set; TEST shows the
coverage, the share asked and the lines the sets really have.

`collect` starts the llama-server of the project (`$LLAMA_SERVER`, or the one smart-commit downloaded) on the GPU
and asks for one token of each Conventional commit's first prompt. Standard library only.
"""

import json
import math
import os
import random
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

TYPES = ["feat", "fix", "docs", "chore", "refactor", "test", "perf", "build", "ci", "style", "revert"]
TOP_LOGPROBS = 12
CALIBRATION_BINS = 10
BANDS = [0.5, 0.7, 0.9]
ASK_BELOW = [0.5, 0.6, 0.7]
BOOTSTRAPS = 1000
SEED = 7
MAX_OFFERED = 4
MIN_OFFERED = 0.02
SPLIT_MASS = 0.85
SPLIT_GAP = 0.25
NLL_FLOOR = 1e-4


def type_of(token: str) -> str | None:
    """The type a token begins (the product's `choices::type_of`): the type, `tests` for `test`, or the only
    type that starts with it."""
    token = token.strip().lower()
    if token == "tests":
        return "test"
    if token in TYPES:
        return token
    if len(token) < 2:
        return None
    starts = [t for t in TYPES if t.startswith(token)]
    return starts[0] if len(starts) == 1 else None


def from_logprobs(candidates: list[tuple[str, float]]) -> list[tuple[str, float]]:
    """The types among the candidates, likeliest first, adding up to 1 (`choices::from_logprobs`)."""
    found: dict[str, float] = {}
    for token, logprob in candidates:
        kind = type_of(token)
        if kind:
            found[kind] = found.get(kind, 0.0) + math.exp(logprob)
    total = sum(found.values())
    if total <= 0:
        return []
    return sorted(((k, p / total) for k, p in found.items()), key=lambda kv: (-kv[1], kv[0]))


def auroc(confidence: list[float], right: list[bool]) -> float:
    """The chance that a commit that is right has a higher confidence than one that is wrong (ties count half)."""
    good = [c for c, r in zip(confidence, right) if r]
    bad = [c for c, r in zip(confidence, right) if not r]
    if not good or not bad:
        return 0.5
    wins = sum((g > b) + 0.5 * (g == b) for g in good for b in bad)
    return wins / (len(good) * len(bad))


def ece(confidence: list[float], right: list[bool], bins: int = CALIBRATION_BINS) -> float:
    """The mean gap between the confidence and the share right, over equal-width bins, weighted by their size."""
    total = len(confidence)
    gap = 0.0
    for b in range(bins):
        low, high = b / bins, (b + 1) / bins
        inside = [(c, r) for c, r in zip(confidence, right) if low <= c < high or (b == bins - 1 and c == 1.0)]
        if inside:
            gap += len(inside) / total * abs(sum(c for c, _ in inside) / len(inside) - sum(r for _, r in inside) / len(inside))
    return gap


def score(cases: list[dict]) -> dict:
    """The measures for `cases` ({real, dist: [[type, p], ...]}); a case with no types counts as wrong."""
    n = len(cases)
    ranked = [[k for k, _ in c["dist"]] for c in cases]
    top = [r[0] if r else None for r in ranked]
    confidence = [c["dist"][0][1] if c["dist"] else 0.0 for c in cases]
    right = [t == c["real"] for t, c in zip(top, cases)]
    out = {"n": n, "top1": sum(right) / n}
    for k in (2, 3):
        out[f"top{k}"] = sum(c["real"] in r[:k] for c, r in zip(cases, ranked)) / n
    out["ece"] = ece(confidence, right)
    out["auroc"] = auroc(confidence, right)
    brier = nll = 0.0
    for c in cases:
        probs = dict(map(tuple, c["dist"]))
        brier += sum((probs.get(t, 0.0) - (t == c["real"])) ** 2 for t in TYPES)
        nll -= math.log(max(probs.get(c["real"], 0.0), NLL_FLOOR))
    out["brier"], out["nll"] = brier / n, nll / n
    edges = [0.0] + BANDS + [1.01]
    out["bins"] = []
    for low, high in zip(edges, edges[1:]):
        inside = [r for c, r in zip(confidence, right) if low <= c < high]
        out["bins"].append({"from": low, "commits": len(inside), "right": sum(inside) / len(inside) if inside else None})
    out["asking"] = []
    for below in ASK_BELOW:
        asked = [i for i, c in enumerate(confidence) if c < below]
        fixed = sum(
            right[i] if i not in asked else cases[i]["real"] in ranked[i][:3] for i in range(n)
        )
        out["asking"].append({"below": below, "asked": len(asked) / n, "type_right": fixed / n})
    return out


def interval(cases: list[dict], measure: str) -> tuple[float, float]:
    """The 95% bootstrap interval of `ece` or `auroc` (resampling the commits)."""
    rng = random.Random(SEED)
    values = []
    for _ in range(BOOTSTRAPS):
        sample = [cases[rng.randrange(len(cases))] for _ in cases]
        conf = [c["dist"][0][1] if c["dist"] else 0.0 for c in sample]
        right = [bool(c["dist"]) and c["dist"][0][0] == c["real"] for c in sample]
        values.append(ece(conf, right) if measure == "ece" else auroc(conf, right))
    values.sort()
    return values[int(0.025 * BOOTSTRAPS)], values[int(0.975 * BOOTSTRAPS) - 1]


def listed_types(system: str) -> list[str]:
    """The types the system message lists as used in the repository (none: every type is allowed)."""
    for line in system.splitlines():
        if line.startswith("- Types used here:"):
            return [t.strip(" .") for t in line.split(":", 1)[1].split(",") if t.strip(" .")]
    return []


def offer(
    dist: list, allowed: list[str], below: float, adaptive: bool = False, cap: int = MAX_OFFERED
) -> list[str] | None:
    """The types to offer (`ask::types_to_offer`), or None when the model is sure enough or has nothing to
    choose between; `adaptive` offers only the two likeliest when the model is split between them; `cap` is the
    most types offered."""
    kept = [(k, p) for k, p in dist if not allowed or k in allowed]
    total = sum(p for _, p in kept)
    if total <= 0:
        return None
    kept = [(k, p / total) for k, p in kept]
    if kept[0][1] >= below:
        return None
    kept = [(k, p) for k, p in kept if p >= MIN_OFFERED][:cap]
    if len(kept) < 2:
        return None
    if adaptive and kept[0][1] + kept[1][1] >= SPLIT_MASS and kept[0][1] - kept[1][1] < SPLIT_GAP:
        kept = kept[:2]
    return [k for k, _ in kept]


def menus(cases: list[dict]) -> list[dict]:
    """For each ASK_BELOW: the asks, the mean lines and the type right of the present menu and the adaptive one."""
    n = len(cases)
    out = []
    for below in ASK_BELOW:
        row = {"below": below}
        for name, adaptive, cap in (("present", False, MAX_OFFERED), ("adaptive", True, MAX_OFFERED), ("cap3", False, 3)):
            asked = right = lines = 0
            for c in cases:
                top = c["dist"][0][0] if c["dist"] else None
                shown = offer(c["dist"], c.get("allowed", []), below, adaptive, cap)
                if shown is None:
                    right += top == c["real"]
                else:
                    asked += 1
                    lines += len(shown)
                    right += c["real"] in shown
            row[name] = {"asked": asked / n, "lines": lines / asked if asked else 0.0, "type_right": right / n}
        out.append(row)
    return out


def ranked(dist: list, allowed: list[str]) -> list[tuple[str, float]]:
    """The types the repository uses, likeliest first, adding up to 1 (as `offer` does)."""
    kept = [(k, p) for k, p in dist if not allowed or k in allowed]
    total = sum(p for _, p in kept)
    return [(k, p / total) for k, p in kept] if total > 0 else []


def mass_above(case: dict) -> float:
    """The probability of the types ranked above the real one (1 when it is not among them)."""
    above = 0.0
    for kind, p in ranked(case["dist"], case.get("allowed", [])):
        if kind == case["real"]:
            return above
        above += p
    return 1.0


def conformal_level(calibration: list[dict], alpha: float) -> float:
    """The ceil((n + 1) * (1 - alpha))-th smallest mass above the real type: sets that keep the types whose
    mass above is at most this hold the real type in at least 1 - alpha of exchangeable commits."""
    scores = sorted(mass_above(c) for c in calibration)
    rank = math.ceil((len(scores) + 1) * (1 - alpha))
    return scores[min(rank, len(scores)) - 1]


def prediction_set(case: dict, level: float) -> list[str]:
    """The types whose probability above them is at most `level` (the likeliest is always in)."""
    out, above = [], 0.0
    for kind, p in ranked(case["dist"], case.get("allowed", [])):
        if above <= level:
            out.append(kind)
        above += p
    return out


def sets(calibration: list[dict], test: list[dict], alpha: float) -> dict:
    """Coverage, share asked (sets of two types or more) and mean lines of those, on `test`."""
    level = conformal_level(calibration, alpha)
    shown = [prediction_set(c, level) for c in test]
    asked = [s for s in shown if len(s) >= 2]
    return {
        "alpha": alpha, "level": level, "coverage": sum(c["real"] in s for c, s in zip(test, shown)) / len(test),
        "asked": len(asked) / len(test), "lines": sum(map(len, asked)) / len(asked) if asked else 0.0,
    }


def server_binary() -> str:
    if os.environ.get("LLAMA_SERVER"):
        return os.environ["LLAMA_SERVER"]
    found = sorted(Path.home().glob(".local/share/smart-commit/llama.cpp/llama-*/llama-server"))
    if not found:
        sys.exit("no llama-server: set LLAMA_SERVER")
    return str(found[-1])


def ask(port: int, system: str, user: str) -> list[tuple[str, float]]:
    body = {
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "max_tokens": 1, "temperature": 0.0, "logprobs": True, "top_logprobs": TOP_LOGPROBS,
    }
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions", json.dumps(body).encode(),
        {"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=120) as reply:
        top = json.load(reply)["choices"][0]["logprobs"]["content"][0]["top_logprobs"]
    return [(entry["token"], entry["logprob"]) for entry in top]


def collect(set_dir: Path, traces: Path, gguf: Path, out: Path, port: int) -> None:
    metas = [json.loads(line) for line in open(set_dir / "meta.jsonl")]
    wanted = {i for i, m in enumerate(metas) if m.get("set") == "conventional" and m.get("type")}
    server = subprocess.Popen(
        [server_binary(), "-m", str(gguf), "-c", "2048", "-ngl", "99", "--port", str(port), "--no-webui"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        for _ in range(240):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2)
                break
            except OSError:
                time.sleep(1)
        else:
            sys.exit("llama-server did not start")
        cases = []
        for index, line in enumerate(open(traces)):
            if index not in wanted:
                continue
            prompt = json.loads(line)["generation"]["attempts"][0]["prompt"]
            real = metas[index]["type"].lower()
            cases.append({
                "line": index, "real": "test" if real == "tests" else real, "allowed": listed_types(prompt["system"]),
                "dist": [[k, p] for k, p in from_logprobs(ask(port, prompt["system"], prompt["user"]))],
            })
        out.write_text(json.dumps({"gguf": gguf.name, "cases": cases}))
    finally:
        server.terminate()
        server.wait()


def report(paths: list[Path]) -> None:
    for path in paths:
        data = json.loads(path.read_text())
        s = score(data["cases"])
        lo_e, hi_e = interval(data["cases"], "ece")
        lo_a, hi_a = interval(data["cases"], "auroc")
        print(f"== {path.stem} ({data['gguf']}, {s['n']} commits)")
        print(f"top1 {s['top1']:.3f}  top2 {s['top2']:.3f}  top3 {s['top3']:.3f}  "
              f"ECE {s['ece']:.3f} [{lo_e:.3f}, {hi_e:.3f}]  AUROC {s['auroc']:.3f} [{lo_a:.3f}, {hi_a:.3f}]  "
              f"Brier {s['brier']:.3f}  NLL {s['nll']:.3f}")
        for b in s["bins"]:
            share = "-" if b["right"] is None else f"{b['right']:.3f}"
            print(f"  confidence from {b['from']:.1f}: {b['commits']:3d} commits, right {share}")
        for a in s["asking"]:
            print(f"  ask below {a['below']:.2f}: asks {a['asked']:.3f}, type right {a['type_right']:.3f}")


def report_menus(paths: list[Path]) -> None:
    for path in paths:
        data = json.loads(path.read_text())
        print(f"== {path.stem} ({len(data['cases'])} commits)")
        for row in menus(data["cases"]):
            p = row["present"]
            print(f"  ask below {row['below']:.2f}: asks {p['asked']:.3f}; present {p['lines']:.2f} lines, type right "
                  f"{p['type_right']:.3f}", end="")
            for name in ("adaptive", "cap3"):
                m = row[name]
                print(f"; {name} {m['lines']:.2f} lines, type right {m['type_right']:.3f}", end="")
            print()


ALPHAS = [0.02, 0.04, 0.06, 0.08, 0.10, 0.12, 0.15, 0.20, 0.25, 0.30]


def report_sets(calibration: Path, test: Path) -> None:
    cal = json.loads(calibration.read_text())["cases"]
    held = json.loads(test.read_text())["cases"]
    print(f"== calibrated on {calibration.stem}, read on {test.stem}")
    for below, present in zip(ASK_BELOW, menus(held)):
        p = present["present"]
        print(f"  present, ask below {below:.2f}: asks {p['asked']:.3f}, {p['lines']:.2f} lines, type right {p['type_right']:.3f}")
    for alpha in ALPHAS:
        r = sets(cal, held, alpha)
        print(f"  sets alpha {alpha:.2f}: level {r['level']:.3f}, asks {r['asked']:.3f}, {r['lines']:.2f} lines, "
              f"type right (coverage) {r['coverage']:.3f}")


def main() -> None:
    args = sys.argv[1:]
    port = 8099
    if "--port" in args:
        at = args.index("--port")
        port = int(args[at + 1])
        del args[at : at + 2]
    if args[:1] == ["collect"]:
        collect(Path(args[1]), Path(args[2]), Path(args[3]), Path(args[4]), port)
    elif args[:1] == ["report"]:
        report([Path(a) for a in args[1:]])
    elif args[:1] == ["sets"]:
        report_sets(Path(args[1]), Path(args[2]))
    elif args[:1] == ["menus"]:
        report_menus([Path(a) for a in args[1:]])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()

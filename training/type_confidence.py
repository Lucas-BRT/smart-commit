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
                "line": index, "real": "test" if real == "tests" else real,
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
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()

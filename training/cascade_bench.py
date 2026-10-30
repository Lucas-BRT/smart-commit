"""Times a cascade between two model sizes on the machine (H15).

The cascade asks the small model first and the big one only when the small one is not sure of its type. The time
of each way to do it is built from what is measured for every commit, with both servers loaded and warm and the
requests of the product (`chat_request`: greedy, seeded, 128 new tokens, no prompt cache):

- `big`: the big model's reply (the baseline, and the cost of an escalation);
- `small`: the small model's reply, with no probabilities;
- `small_lp`: the small model's reply with the probabilities of its tokens: the confidence is in the first one;
- `probe`: the small model's first token alone (what `model.ask_type` reads).

Designs, for a commit with confidence `conf` and a limit `tau`:

- `big alone`: `big`;
- `small alone`: `small`;
- `single pass`: `small_lp`, and `big` when `conf < tau` (the small reply is dropped);
- `probe, then reply`: `probe`, then `small` when `conf >= tau` and `big` otherwise, which reads the prompt twice.

The loads of the servers (`--no-repack --fit off`, as the product's fast load) are measured apart, for the case
where a model is not kept loaded.

    python -I cascade_bench.py run SET_DIR TRACES.jsonl SMALL.gguf BIG.gguf OUT.json [--device vulkan|cpu] [--limit N]
    python -I cascade_bench.py report OUT.json [--tau 0.5]

`TRACES.jsonl` are `smart-commit eval` traces of SET_DIR (their first prompt of each Conventional commit is the one
timed). Standard library only; `$LLAMA_SERVER` or the server smart-commit downloaded.
"""

import json
import math
import statistics
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from type_confidence import TOP_LOGPROBS, from_logprobs, server_binary  # noqa: E402

MAX_NEW_TOKENS = 128
CONTEXT = 1024 + 1024 // 2 + MAX_NEW_TOKENS     # the product's: prompt budget, half again, and the reply
FAST_LOAD_FLAGS = ["--no-repack", "--fit", "off"]
WARMUP = 3
LOADS = 3
TAU = 0.5


def body(prompt: dict, max_tokens: int = MAX_NEW_TOKENS, logprobs: bool = False) -> dict:
    """The product's request (`chat_request`), with the probabilities of the tokens when asked."""
    request = {
        "messages": [
            {"role": "system", "content": prompt["system"]},
            {"role": "user", "content": prompt["user"]},
        ],
        "max_tokens": max_tokens,
        "temperature": 0,
        "top_k": 1,
        "seed": 0,
        "cache_prompt": False,
    }
    if logprobs:
        request["logprobs"] = True
        request["top_logprobs"] = TOP_LOGPROBS
    return request


def reply_text(reply: dict) -> str:
    return reply["choices"][0]["message"]["content"]


def confidence(reply: dict) -> tuple[str | None, float]:
    """The likeliest type of the reply's first token and its probability among the types (0 when none)."""
    content = reply["choices"][0].get("logprobs", {}).get("content") or []
    if not content:
        return None, 0.0
    types = from_logprobs([(t["token"], t["logprob"]) for t in content[0]["top_logprobs"]])
    return (types[0][0], types[0][1]) if types else (None, 0.0)


def reply_type(line: str) -> str | None:
    """The type a reply's first line starts with (`tests` is `test`), lowercase."""
    head = line.split(":", 1)[0].split("(", 1)[0].strip().rstrip("!").lower()
    if not head or " " in head or ":" not in line:
        return None
    return "test" if head == "tests" else head


def percentile(values: list[float], share: float) -> float:
    """The value `share` of the way up the sorted values (nearest rank)."""
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, max(0, math.ceil(share * len(ordered)) - 1))]


def stats(values: list[float]) -> dict:
    return {"mean": statistics.fmean(values), "p50": percentile(values, 0.5), "p90": percentile(values, 0.9)}


def designs(rows: list[dict], tau: float) -> dict:
    """The time per commit of each design, and the share the big model answers."""
    sure = [r["conf"] >= tau for r in rows]
    return {
        "escalated": sum(not s for s in sure) / len(rows),
        "big alone": [r["big"] for r in rows],
        "small alone": [r["small"] for r in rows],
        "single pass": [r["small_lp"] + (0.0 if s else r["big"]) for r, s in zip(rows, sure)],
        "probe, then reply": [r["probe"] + (r["small"] if s else r["big"]) for r, s in zip(rows, sure)],
    }


def cold(rows: list[dict], tau: float, load_small: float, load_big: float) -> dict:
    """The time per commit when each message starts its servers: the big model alone loads the big one, the cascade
    loads the small one and the big one only when it escalates."""
    d = designs(rows, tau)
    sure = [r["conf"] >= tau for r in rows]
    return {
        "big alone": [load_big + t for t in d["big alone"]],
        "single pass": [load_small + t + (0.0 if s else load_big) for t, s in zip(d["single pass"], sure)],
    }


class Server:
    """A llama-server of the product's flags on a loopback port."""

    def __init__(self, gguf: Path, port: int, device: str) -> None:
        layers, dev = ("999", "Vulkan0") if device == "vulkan" else ("0", "none")
        self.port = port
        self.args = [
            server_binary(), "--model", str(gguf), "--host", "127.0.0.1", "--port", str(port),
            "--n-gpu-layers", layers, "--device", dev, "--ctx-size", str(CONTEXT), "--parallel", "1",
            "--no-webui", "--offline", *FAST_LOAD_FLAGS,
        ]
        self.process: subprocess.Popen | None = None
        self.load_seconds = 0.0

    def __enter__(self) -> "Server":
        started = time.perf_counter()
        self.process = subprocess.Popen(self.args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(600):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{self.port}/health", timeout=2).read()
                self.load_seconds = time.perf_counter() - started
                return self
            except (urllib.error.URLError, OSError):
                time.sleep(0.1)
        self.__exit__()
        raise RuntimeError(f"llama-server did not start: {' '.join(self.args)}")

    def __exit__(self, *_) -> None:
        if self.process:
            self.process.terminate()
            self.process.wait()

    def post(self, request: dict) -> tuple[dict, float]:
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/v1/chat/completions", json.dumps(request).encode(),
            {"Content-Type": "application/json"},
        )
        started = time.perf_counter()
        with urllib.request.urlopen(req, timeout=300) as reply:
            data = json.load(reply)
        return data, time.perf_counter() - started


def commits(set_dir: Path, traces: Path, limit: int | None) -> list[dict]:
    """The Conventional commits of the set with their first prompt."""
    metas = [json.loads(line) for line in open(set_dir / "meta.jsonl")]
    out = []
    for index, line in enumerate(open(traces)):
        meta = metas[index]
        if meta.get("set") != "conventional" or not meta.get("type"):
            continue
        out.append({"line": index, "real": meta["type"].lower(), "prompt": json.loads(line)["generation"]["attempts"][0]["prompt"]})
        if limit and len(out) >= limit:
            break
    return out


def run(set_dir: Path, traces: Path, small: Path, big: Path, out: Path, device: str, limit: int | None) -> None:
    work = commits(set_dir, traces, limit)
    loads = {"small": [], "big": []}
    for name, gguf in (("small", small), ("big", big)):
        for _ in range(LOADS):
            with Server(gguf, 8111, device) as s:
                loads[name].append(s.load_seconds)
    print(f"loads: small {loads['small']}, big {loads['big']}", flush=True)
    rows = []
    with Server(small, 8112, device) as s, Server(big, 8113, device) as b:
        for c in work[:WARMUP]:
            s.post(body(c["prompt"])), b.post(body(c["prompt"]))
        for n, c in enumerate(work, 1):
            _, probe = s.post(body(c["prompt"], max_tokens=1, logprobs=True))
            lp_reply, small_lp = s.post(body(c["prompt"], logprobs=True))
            small_reply, small_t = s.post(body(c["prompt"]))
            big_reply, big_t = b.post(body(c["prompt"]))
            kind, conf = confidence(lp_reply)
            rows.append({
                "line": c["line"], "real": c["real"], "probe": probe, "small_lp": small_lp, "small": small_t,
                "big": big_t, "conf": conf, "small_type": kind,
                "small_reply": reply_text(small_reply).splitlines()[0] if reply_text(small_reply) else "",
                "big_reply": reply_text(big_reply).splitlines()[0] if reply_text(big_reply) else "",
            })
            if n % 25 == 0:
                print(f"{n}/{len(work)}", flush=True)
    out.write_text(json.dumps({"device": device, "loads": loads, "rows": rows}))


def report(path: Path, tau: float) -> None:
    data = json.loads(path.read_text())
    rows, loads = data["rows"], data["loads"]
    load_small, load_big = statistics.median(loads["small"]), statistics.median(loads["big"])
    d = designs(rows, tau)
    print(f"{len(rows)} commits on {data['device']}, tau {tau}: the big model answers {d['escalated']:.3f}")
    print(f"loads (median of {len(loads['big'])}): small {load_small:.2f} s, big {load_big:.2f} s")
    print("both servers loaded:")
    base = stats(d["big alone"])
    for name in ("big alone", "small alone", "single pass", "probe, then reply"):
        s = stats(d[name])
        print(f"  {name:18s} mean {s['mean']:.2f} s  p50 {s['p50']:.2f}  p90 {s['p90']:.2f}   ({s['mean'] / base['mean'] - 1:+.1%} against the big model)")
    c = cold(rows, tau, load_small, load_big)
    print("each message loads what it uses:")
    base_cold = stats(c["big alone"])
    for name in ("big alone", "single pass"):
        s = stats(c[name])
        print(f"  {name:18s} mean {s['mean']:.2f} s  p50 {s['p50']:.2f}  p90 {s['p90']:.2f}   ({s['mean'] / base_cold['mean'] - 1:+.1%})")
    right = lambda texts: sum(reply_type(t) == r["real"] for t, r in zip(texts, rows)) / len(rows)  # noqa: E731
    chosen = [r["small_reply"] if r["conf"] >= tau else r["big_reply"] for r in rows]
    print(f"first line's type matches: small {right([r['small_reply'] for r in rows]):.3f}, big "
          f"{right([r['big_reply'] for r in rows]):.3f}, cascade {right(chosen):.3f}")


def main() -> None:
    args = sys.argv[1:]
    flag = lambda name, default=None: args[args.index(name) + 1] if name in args else default  # noqa: E731
    if args[:1] == ["run"]:
        run(Path(args[1]), Path(args[2]), Path(args[3]), Path(args[4]), Path(args[5]), flag("--device", "vulkan"),
            int(flag("--limit")) if flag("--limit") else None)
    elif args[:1] == ["report"]:
        report(Path(args[1]), float(flag("--tau", TAU)))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()

"""Selected inputs -> the replies a model samples for them, to mine
preference pairs (`sc-training pairs` judges them).

Builds the prompts with `smart-commit prompt --inputs` (empty
XDG_CONFIG_HOME, as `make_examples.py`), drops the commits of the
repositories of the evaluation sets, and samples SAMPLES replies per input
from a llama-server with fixed seeds, so the same model, server and inputs
give the same candidates. Writes to OUT `inputs.jsonl` and `meta.jsonl` (what
is left of SRC) and `candidates.jsonl`: `{"system", "user", "replies"}` per
line, in the same order. Stopped, it resumes where it was.

    python -I mine_pairs.py SRC OUT SMART_COMMIT GGUF --server LLAMA_SERVER [options]
    python -I mine_pairs.py SRC OUT SMART_COMMIT GGUF --url http://127.0.0.1:8080

`--rules` and `--diff` are those the model was trained on: `full` and
`plain` for v0.1.
"""

import argparse
import json
import queue
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
from pathlib import Path

MAX_TOKENS = 64
# A reply of the 1.5B model's prompt budget, plus half again, and the reply.
CONTEXT_PER_SLOT = 1664


def read_lines(path: Path) -> list[str]:
    return path.read_text().splitlines() if path.exists() else []


def repos_of(directories: list[Path]) -> set[str]:
    """The repositories of the `meta.jsonl` of each of `directories`."""
    return {
        json.loads(line)["repo"]
        for d in directories
        for line in read_lines(d / "meta.jsonl")
    }


def select(src: Path, out: Path, exclude: set[str], limit: int) -> int:
    """Writes to `out` the inputs of `src` outside `exclude`; how many."""
    inputs = read_lines(src / "inputs.jsonl")
    meta = read_lines(src / "meta.jsonl")
    assert len(inputs) == len(meta), f"{len(inputs)} inputs, {len(meta)} meta"
    kept = [(i, m) for i, m in zip(inputs, meta) if json.loads(m)["repo"] not in exclude]
    if limit:
        kept = kept[:limit]
    out.mkdir(parents=True, exist_ok=True)
    (out / "inputs.jsonl").write_text("".join(i + "\n" for i, _ in kept))
    (out / "meta.jsonl").write_text("".join(m + "\n" for _, m in kept))
    return len(kept)


def prompts(binary: str, inputs: Path, rules: str, diff: str) -> list[dict]:
    with tempfile.TemporaryDirectory() as config_home:
        env = {"XDG_CONFIG_HOME": config_home, "PATH": "/usr/bin:/bin"}
        lines = subprocess.run(
            [binary, "prompt", "--inputs", str(inputs), "--rules", rules, "--diff", diff],
            env=env, check=True, capture_output=True, text=True,
        ).stdout.splitlines()
    return [json.loads(line) for line in lines]


def sample(url: str, prompt: dict, samples: int, temperature: float, top_p: float,
           seed: int) -> list[str]:
    """`samples` replies to `prompt`, with seeds SEED, SEED + 1, ..."""
    replies = []
    for k in range(samples):
        body = {
            "messages": [
                {"role": "system", "content": prompt["system"]},
                {"role": "user", "content": prompt["user"]},
            ],
            "max_tokens": MAX_TOKENS,
            "temperature": temperature,
            "top_p": top_p,
            "seed": seed + k,
            "cache_prompt": True,
        }
        request = urllib.request.Request(
            url + "/v1/chat/completions", json.dumps(body).encode(),
            {"content-type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=300) as response:
            reply = json.load(response)["choices"][0]["message"]["content"]
        replies.append(reply)
    return replies


def mine(out: Path, binary: str, url: str, rules: str, diff: str, samples: int,
         temperature: float, top_p: float, seed: int, workers: int) -> int:
    """Samples the replies for the inputs in `out` not done yet; how many."""
    built = prompts(binary, out / "inputs.jsonl", rules, diff)
    done = len(read_lines(out / "candidates.jsonl"))
    todo: queue.Queue = queue.Queue()
    for n in range(done, len(built)):
        todo.put(n)
    results: dict[int, list[str]] = {}
    lock = threading.Lock()

    def work() -> None:
        while True:
            try:
                n = todo.get_nowait()
            except queue.Empty:
                return
            replies = sample(url, built[n], samples, temperature, top_p, seed + 1000 * n)
            with lock:
                results[n] = replies

    threads = [threading.Thread(target=work) for _ in range(workers)]
    for t in threads:
        t.start()
    written = done
    started = time.time()
    with open(out / "candidates.jsonl", "a") as f:
        # In order, as they finish: a stop leaves a prefix to resume from.
        while written < len(built):
            with lock:
                replies = results.pop(written, None)
            if replies is None:
                if not any(t.is_alive() for t in threads) and written not in results:
                    raise RuntimeError(f"the workers stopped before input {written}")
                time.sleep(0.05)
                continue
            row = {**built[written], "replies": replies}
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
            f.flush()
            written += 1
            if written % 50 == 0:
                print(f"{written}/{len(built)} in {time.time() - started:.0f} s", flush=True)
    for t in threads:
        t.join()
    return len(built) - done


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def serve(server: str, gguf: str, device: str, workers: int) -> tuple[subprocess.Popen, str]:
    port = free_port()
    args = [
        server, "--model", gguf, "--host", "127.0.0.1", "--port", str(port),
        "--n-gpu-layers", "0" if device == "cpu" else "999",
        "--device", "none" if device == "cpu" else device,
        "--ctx-size", str(CONTEXT_PER_SLOT * workers), "--parallel", str(workers),
        "--no-webui", "--offline",
        # Every prompt is new, so the prompt cache would only fill the RAM (8 GiB by default).
        "--cache-ram", "0",
    ]
    process = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    url = f"http://127.0.0.1:{port}"
    for _ in range(600):
        if process.poll() is not None:
            raise RuntimeError(f"{server} exited with {process.returncode}")
        try:
            with urllib.request.urlopen(url + "/health", timeout=2):
                return process, url
        except OSError:
            time.sleep(0.5)
    process.kill()
    raise RuntimeError("llama-server was not ready after 300 s")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("src", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("binary", help="smart-commit, to build the prompts")
    parser.add_argument("gguf", help="the model to sample from")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--server", help="llama-server to start for GGUF")
    group.add_argument("--url", help="a llama-server already running")
    parser.add_argument("--device", default="Vulkan0",
                        help="`cpu`, or a llama.cpp device such as Vulkan0 or CUDA0")
    parser.add_argument("--rules", choices=["full", "omitted"], default="full")
    parser.add_argument("--diff", choices=["plain", "annotated"], default="plain")
    parser.add_argument("--samples", type=int, default=4)
    parser.add_argument("--temperature", type=float, default=0.7)
    parser.add_argument("--top-p", type=float, default=0.95)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--limit", type=int, default=0, help="first N inputs only")
    parser.add_argument("--exclude-repos-of", type=Path, nargs="*", default=[],
                        metavar="DIR", help="evaluation sets whose repositories are left out")
    args = parser.parse_args()
    kept = select(args.src, args.out, repos_of(args.exclude_repos_of), args.limit)
    print(f"{kept} inputs from {args.src}")
    process = None
    url = args.url
    try:
        if args.server:
            process, url = serve(args.server, args.gguf, args.device, args.workers)
        new = mine(args.out, args.binary, url, args.rules, args.diff, args.samples,
                   args.temperature, args.top_p, args.seed, args.workers)
        print(f"{new} inputs sampled, {kept - new} were done")
    finally:
        if process:
            process.terminate()
            process.wait()


if __name__ == "__main__":
    main()

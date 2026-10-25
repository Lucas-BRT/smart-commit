"""Packs a mining run for the Kaggle notebook (kaggle_mine.ipynb) into one
directory, to upload as a private Kaggle dataset.

    python -I kaggle_mine_bundle.py OUT_DIR SRC_DIR [--shards N] [--samples K] [--seed S] [MODEL ...]

SRC_DIR is what `mine_pairs.py --prompts-only` wrote: `inputs.jsonl`,
`meta.jsonl` and `prompts.jsonl`. The inputs go into N shards, every Nth
input to each, so a shard that does not finish leaves the others
representative; each MODEL (`0.5b`, `1.5b`) samples every shard, a job per
pair, the longest first. The bundle holds mine_pairs.py, the shards,
and `runs.json` with what the notebook downloads, pinned: llama.cpp's CUDA
build by SHA-256, the models by the revision and SHA-256 of
`src/models/registry.rs`.
"""

import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path

HERE = Path(__file__).parent
USER = "lucasbrt"  # Kaggle username
REGISTRY = HERE.parent / "src/models/registry.rs"
# The CUDA 12.8 build Kaggle's T4 driver runs, and the runtime libraries it needs.
LLAMA = {
    "release": "b11381",
    "assets": [
        {"name": "llama-b11381-bin-ubuntu-cuda-12.8-x64.tar.gz",
         "sha256": "86805e29e1277297334d20bcb2a75cd6d921d0b8fcc22d521d5c3961f3990fbb"},
        {"name": "cudart-llama-b11381-bin-ubuntu-cuda-12.8-x64.tar.gz",
         "sha256": "13c6d5079689e8c0bc90fc4b89188a5d9777cc2f362398f3376fd4c8d5c3c044"},
    ],
    "url": "https://github.com/ggml-org/llama.cpp/releases/download/{release}/{name}",
}
# Seconds a job takes per input and sample on a T4, to put the longest first.
COST = {"0.5b": 1, "1.5b": 2.2}


def models(registry: str) -> dict[str, dict]:
    """The v0.1 models of the registry: repo, revision, file, size, sha256."""
    found = {}
    for block in registry.split("Model {")[1:]:
        fields = dict(re.findall(r'\b(id|repo|revision|file|sha256): "([^"]*)"', block))
        size = re.search(r"size: ([\d_]+)", block)
        if fields.get("id", "").startswith("smart-commit-") and size:
            key = fields["id"].split("-")[2]
            found[key] = {"repo": fields["repo"], "revision": fields["revision"],
                          "file": fields["file"], "size": int(size[1].replace("_", "")),
                          "sha256": fields["sha256"]}
    return found


def shards(src: Path, out: Path, count: int) -> list[dict]:
    """Writes `shard-K` for K in 0..count: every count-th line of the three files."""
    files = {name: (src / f"{name}.jsonl").read_text().splitlines()
             for name in ("inputs", "meta", "prompts")}
    sizes = {len(lines) for lines in files.values()}
    if len(sizes) != 1:
        raise SystemExit(f"{src}: inputs, meta and prompts must have the same number of lines")
    written = []
    for k in range(count):
        directory = out / f"shard-{k}"
        directory.mkdir(parents=True, exist_ok=True)
        for name, lines in files.items():
            (directory / f"{name}.jsonl").write_text("".join(f"{line}\n" for line in lines[k::count]))
        written.append({"dir": directory.name, "inputs": len(files["inputs"][k::count]),
                        "sha256": hashlib.sha256((directory / "prompts.jsonl").read_bytes()).hexdigest()})
    return written


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("out", type=Path)
    parser.add_argument("src", type=Path)
    parser.add_argument("--shards", type=int, default=4)
    parser.add_argument("--samples", type=int, default=6)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("models", nargs="*", default=["0.5b", "1.5b"])
    args = parser.parse_args()

    pinned = models(REGISTRY.read_text())
    unknown = [m for m in args.models if m not in pinned]
    if unknown:
        parser.error(f"not in the registry: {unknown}; it has {sorted(pinned)}")
    args.out.mkdir(parents=True, exist_ok=True)
    shutil.copy(HERE / "mine_pairs.py", args.out / "mine_pairs.py")
    parts = shards(args.src, args.out, args.shards)
    jobs = [{"name": f"{model}-{part['dir']}", "model": model, "shard": part["dir"],
             "inputs": part["inputs"], "prompts_sha256": part["sha256"]}
            for model in args.models for part in parts]
    jobs.sort(key=lambda j: -j["inputs"] * COST[j["model"]])
    (args.out / "runs.json").write_text(json.dumps({
        "llama": LLAMA,
        "models": {m: pinned[m] for m in args.models},
        "sampling": {"rules": "full", "diff": "plain", "samples": args.samples,
                     "temperature": 0.7, "top_p": 0.95, "seed": args.seed, "workers": 4},
        "jobs": jobs,
    }, indent=2) + "\n")
    (args.out / "dataset-metadata.json").write_text(json.dumps({
        "title": f"smart-commit-training-{args.out.name}",
        "id": f"{USER}/smart-commit-training-{args.out.name}",
        "licenses": [{"name": "other"}],
    }, indent=2) + "\n")
    for job in jobs:
        print(f"{job['name']}: {job['inputs']} inputs")
    print(f"upload {args.out} as a private Kaggle dataset")


if __name__ == "__main__":
    main()

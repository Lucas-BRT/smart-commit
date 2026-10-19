"""Packs runs for the Kaggle notebook (kaggle_train.ipynb) into one
directory, to upload as a private Kaggle dataset.

    python -I kaggle_bundle.py OUT_DIR NAME=MODEL:EXAMPLES.jsonl[:TRAIN_ARGS] ...

MODEL is a key of BASES. e.g. `h2=0.5b:data/h2/examples.jsonl:--rank 32`
adds a run `h2` on the 0.5B with `train.py ... --rank 32` (and Kaggle's
precision flags). The bundle holds
train.py, the examples, `runs.json` and `requirements.txt` (our uv.lock
versions, without torch: Kaggle's own CUDA build is kept, and run.json
records its version).
"""

import hashlib
import json
import shutil
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).parent
USER = "lucasbrt"  # Kaggle username
BASES = {
    # The revision scripts/export-model.sh pins.
    "1.5b": {"repo": "Qwen/Qwen2.5-Coder-1.5B-Instruct",
             "revision": "2e1fd397ee46e1388853d2af2c993145b0f1098a"},
    "0.5b": {"repo": "Qwen/Qwen2.5-Coder-0.5B-Instruct",
             "revision": "ea3f2471cf1b1f0db85067f1ef93848e38e88c25"},
}
PACKAGES = ["transformers", "tokenizers", "safetensors", "huggingface-hub", "peft",
            "accelerate", "bitsandbytes"]
# T4 and P100 have no bf16: 4-bit base with fp16 compute.
KAGGLE_ARGS = ["--quant", "4bit", "--dtype", "fp16", "--resume"]


def locked_versions() -> dict[str, str]:
    lock = tomllib.loads((HERE / "uv.lock").read_text())
    return {p["name"]: p["version"] for p in lock["package"]}


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    shutil.copy(HERE / "train.py", out / "train.py")
    versions = locked_versions()
    (out / "requirements.txt").write_text(
        "".join(f"{p}=={versions[p]}\n" for p in PACKAGES)
    )
    runs = []
    for spec in sys.argv[2:]:
        name, rest = spec.split("=", 1)
        model, _, rest = rest.partition(":")
        examples, _, extra = rest.partition(":")
        target = f"{name}.examples.jsonl"
        shutil.copy(examples, out / target)
        digest = hashlib.sha256((out / target).read_bytes()).hexdigest()
        runs.append({"name": name, "base": BASES[model], "examples": target, "sha256": digest,
                     "args": KAGGLE_ARGS + extra.split()})
    (out / "runs.json").write_text(json.dumps({"runs": runs}, indent=2) + "\n")
    (out / "dataset-metadata.json").write_text(json.dumps({
        "title": f"smart-commit-training-{out.name}",
        "id": f"{USER}/smart-commit-training-{out.name}",
        "licenses": [{"name": "other"}],
    }, indent=2) + "\n")
    for run in runs:
        print(f"{run['name']}: {run['examples']} {' '.join(run['args'])}")
    print(f"upload {out} as a private Kaggle dataset")


if __name__ == "__main__":
    main()

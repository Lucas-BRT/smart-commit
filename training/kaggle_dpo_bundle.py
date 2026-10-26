"""Packs DPO runs for the Kaggle notebook (kaggle_dpo.ipynb) into one
directory, to upload as a private Kaggle dataset.

    python -I kaggle_dpo_bundle.py OUT_DIR [--per-gpu N] NAME=MODEL:SFT_ADAPTER:TRAIN.jsonl:VAL.jsonl[:TRAIN_ARGS] ...

MODEL is a key of BASES, SFT_ADAPTER the directory train.py wrote, TRAIN and
VAL the files of `sc-training balance`. e.g.
`b1=0.5b:runs/kaggle-v2/c40k-0.5b/adapter:t.jsonl:v.jsonl:--beta 0.3` adds a
run `b1` of `train_dpo.py ... --beta 0.3` (and Kaggle's precision flag).
`--per-gpu N` lets the notebook run N of them at once on each GPU: a run of the
0.5B uses under 2 GiB of a T4's 15, and whether the others slow it down is
for the logs to say. The
bundle holds train.py and train_dpo.py, what each run reads (the pairs and
only the two files of its adapter that matter), `runs.json` and
`requirements.txt` (our uv.lock versions, without torch: Kaggle's own CUDA
build is kept, and run.json records its version).
"""

import hashlib
import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kaggle_bundle import BASES, HERE, PACKAGES, USER, locked_versions  # noqa: E402

# T4 and P100 have no bf16.
KAGGLE_ARGS = ["--dtype", "fp16"]
ADAPTER_FILES = ["adapter_config.json", "adapter_model.safetensors"]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def add_run(out: Path, spec: str) -> dict:
    """Copies what the run reads into `out` and describes it for the notebook."""
    name, rest = spec.split("=", 1)
    model, adapter, train, val, *extra = rest.split(":", 4)
    if model not in BASES:
        raise SystemExit(f"{name}: no base model {model!r}; the bundle knows {sorted(BASES)}")
    files = {}
    for kind, source in (("train", Path(train)), ("val", Path(val))):
        files[kind] = f"{name}.{kind}.jsonl"
        shutil.copy(source, out / files[kind])
    sft = out / f"sft-{Path(adapter).parent.name}"
    sft.mkdir(exist_ok=True)
    for file in ADAPTER_FILES:
        shutil.copy(Path(adapter) / file, sft / file)
    return {"name": name, "base": BASES[model], "sft": sft.name,
            "sft_sha256": sha256(sft / "adapter_model.safetensors"),
            "train": files["train"], "train_sha256": sha256(out / files["train"]),
            "val": files["val"], "args": KAGGLE_ARGS + (extra[0].split() if extra else [])}


def main() -> None:
    out, specs, per_gpu = Path(sys.argv[1]), sys.argv[2:], 1
    if specs[:1] == ["--per-gpu"]:
        per_gpu, specs = int(specs[1]), specs[2:]
    if not specs or per_gpu < 1:
        raise SystemExit(__doc__)
    out.mkdir(parents=True, exist_ok=True)
    for script in ("train.py", "train_dpo.py"):
        shutil.copy(HERE / script, out / script)
    versions = locked_versions()
    (out / "requirements.txt").write_text("".join(f"{p}=={versions[p]}\n" for p in PACKAGES))
    runs = [add_run(out, spec) for spec in specs]
    (out / "runs.json").write_text(json.dumps({"per_gpu": per_gpu, "runs": runs}, indent=2) + "\n")
    (out / "dataset-metadata.json").write_text(json.dumps({
        "title": f"smart-commit-training-{out.name}",
        "id": f"{USER}/smart-commit-training-{out.name}",
        "licenses": [{"name": "other"}],
    }, indent=2) + "\n")
    for run in runs:
        print(f"{run['name']}: {run['train']} {' '.join(run['args'])}")
    print(f"upload {out} as a private Kaggle dataset")


if __name__ == "__main__":
    main()

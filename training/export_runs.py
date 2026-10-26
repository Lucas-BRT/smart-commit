"""Exports every DPO run of a directory to a GGUF, and prints `NAME=GGUF` for
each, the arguments `compare_models.py` takes.

    python -I export_runs.py BASE_DIR SFT_ADAPTER RUNS_DIR --llama-src DIR --llama-bin DIR

A run is a directory of RUNS_DIR with an `adapter` (what `kaggle_dpo.ipynb`
packs); its GGUF is `RUNS_DIR/NAME/model-q4_k_m.gguf`, made by
`export_gguf.py` with the SFT adapter first and the run's after, and kept: a
run already exported is not made again. Messages go to stderr, so
`compare_models.py ... $(python -I export_runs.py ...)` works.
"""

import argparse
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent


def runs(directory: Path) -> list[str]:
    """The names of the runs of `directory`, in order."""
    return sorted(p.name for p in directory.iterdir() if (p / "adapter").is_dir())


def export(name: str, args: argparse.Namespace) -> Path:
    gguf = args.runs / name / "model-q4_k_m.gguf"
    if gguf.exists():
        print(f"{name}: already exported", file=sys.stderr)
        return gguf
    print(f"{name}: exporting", file=sys.stderr)
    done = subprocess.run(
        [sys.executable, "-I", str(args.exporter), str(args.base), str(gguf),
         "--adapter", str(args.sft), "--adapter", str(args.runs / name / "adapter"),
         "--llama-src", str(args.llama_src), "--llama-bin", str(args.llama_bin)],
        capture_output=True, text=True)
    if done.returncode or not gguf.exists():
        gguf.unlink(missing_ok=True)
        raise SystemExit(f"{name}: the export failed ({done.returncode}):\n{done.stderr[-1500:]}")
    return gguf


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("base", type=Path)
    parser.add_argument("sft", type=Path, help="the SFT adapter the runs were trained on")
    parser.add_argument("runs", type=Path)
    parser.add_argument("--llama-src", type=Path, required=True)
    parser.add_argument("--llama-bin", type=Path, required=True)
    parser.add_argument("--exporter", type=Path, default=HERE / "export_gguf.py", help=argparse.SUPPRESS)
    args = parser.parse_args()
    names = runs(args.runs)
    if not names:
        raise SystemExit(f"no run with an adapter in {args.runs}")
    for name in names:
        print(f"{name}={export(name, args).resolve()}")


if __name__ == "__main__":
    main()

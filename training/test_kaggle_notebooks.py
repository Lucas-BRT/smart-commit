"""Checks the code of the Kaggle notebooks that cannot run anywhere but on Kaggle
without these stand-ins: the scheduler cells, and what a kernel must do when its
runs fail. (The bundles the notebooks read are made by `sc-training kaggle-bundle`
and tested in Rust.)

    python -I test_kaggle_notebooks.py
"""

import hashlib
import json
import pathlib
import tempfile
import time
import types
from pathlib import Path

HERE = Path(__file__).parent


def run_scheduler(per_gpu: int, gpus: int, runs: int) -> tuple[list[tuple[int, int]], list[str]]:
    """Runs the notebook's scheduler cell with stand-in processes that take three
    checks to end; returns, for each start, (gpu, runs then going on it) and the
    archives made."""
    cells = json.loads((HERE / "kaggle_dpo.ipynb").read_text())["cells"]
    source = next("".join(c["source"]) for c in cells if "pending, running" in "".join(c["source"]))
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        bundle, work = d / "bundle", d / "working"
        (bundle / "sft").mkdir(parents=True)
        work.mkdir()
        (bundle / "sft/adapter_model.safetensors").write_bytes(b"w")
        (bundle / "t.jsonl").write_bytes(b"t")
        digest = lambda b: hashlib.sha256(b).hexdigest()  # noqa: E731
        spec = {"per_gpu": per_gpu, "runs": [
            {"name": f"r{n}", "base": {"repo": "o/m", "revision": "r"}, "sft": "sft", "sft_sha256": digest(b"w"),
             "train": "t.jsonl", "train_sha256": digest(b"t"), "val": "t.jsonl", "args": []} for n in range(runs)]}
        if per_gpu == 1:
            del spec["per_gpu"]  # the default is one
        started: list[tuple[int, int]] = []
        going: dict[int, int] = {}

        class Process:
            def __init__(self, cmd, stdout, stderr, env):
                self.gpu, self.checks = int(env["CUDA_VISIBLE_DEVICES"]), 0
                going[self.gpu] = going.get(self.gpu, 0) + 1
                started.append((self.gpu, going[self.gpu]))
                self.out = Path(cmd[6])  # python -I train_dpo.py TRAIN BASE SFT OUT --val ...
                (self.out / "adapter").mkdir(parents=True, exist_ok=True)

            def poll(self):
                self.checks += 1
                if self.checks < 3:
                    return None
                going[self.gpu] -= 1
                self.returncode = 0
                return 0

        def tar(command, check):
            Path(command[2]).write_bytes(b"archive")

        fake = types.SimpleNamespace(Popen=Process, run=tar, STDOUT=-2)
        sleep, time.sleep = time.sleep, lambda seconds: None
        try:
            namespace = {"pathlib": pathlib, "json": json, "os": __import__("os"), "subprocess": fake,
                         "bundle": bundle, "spec": spec, "bases": {"o/m@r": "/base"},
                         "torch": types.SimpleNamespace(cuda=types.SimpleNamespace(device_count=lambda: gpus))}
            exec(source.replace("/kaggle/working", str(work)), namespace)
        finally:
            time.sleep = sleep
        return started, sorted(p.name for p in work.glob("*.tar.gz"))


def test_the_scheduler_keeps_per_gpu_runs_going_on_each_gpu() -> None:
    # One at a time, as before: 2 GPUs, 6 runs, never two on a GPU.
    started, archives = run_scheduler(per_gpu=1, gpus=2, runs=6)
    assert max(n for _, n in started) == 1 and len(started) == 6
    assert archives == [f"r{n}.tar.gz" for n in range(6)]
    # Three at a time: the six start together, three on each GPU, none beyond.
    started, archives = run_scheduler(per_gpu=3, gpus=2, runs=6)
    assert sorted(started[:6]) == [(0, 1), (0, 2), (0, 3), (1, 1), (1, 2), (1, 3)]
    assert max(n for _, n in started) == 3
    assert len(archives) == 6
    # More runs than slots: a new one starts when one ends, never above the limit.
    started, archives = run_scheduler(per_gpu=2, gpus=1, runs=5)
    assert max(n for _, n in started) == 2 and len(archives) == 5


def test_the_notebook_removes_kaggles_torchao_and_fails_when_a_run_does() -> None:
    """Kaggle's torchao 0.10 made peft 0.21 refuse every 16-bit layer, all six runs
    exited with 1, and the kernel still said COMPLETE."""
    cells = json.loads((HERE / "kaggle_dpo.ipynb").read_text())["cells"]
    code = ["".join(c["source"]) for c in cells if c["cell_type"] == "code"]
    install = next(c for c in code if "pip install" in c)
    assert install.index("pip uninstall -y -q torchao") < install.index("pip install -q -r")
    scheduler = next(c for c in code if "pending, running" in c)
    assert "assert not failed" in scheduler.splitlines()[-1]


def test_the_mining_notebook_fails_when_a_job_does() -> None:
    """A kernel whose jobs failed still says COMPLETE: only an exception makes it ERROR."""
    cells = json.loads((HERE / "kaggle_mine.ipynb").read_text())["cells"]
    scheduler = next("".join(c["source"]) for c in cells if "pending, running, attempts" in "".join(c["source"]))
    assert "assert not failed" in scheduler.splitlines()[-1]


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

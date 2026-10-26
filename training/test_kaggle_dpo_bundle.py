"""Checks the bundle of the Kaggle DPO runs.

    python -I test_kaggle_dpo_bundle.py
"""

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import types
from pathlib import Path

HERE = Path(__file__).parent


def test_a_run_carries_what_it_reads_and_nothing_else() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        adapter = d / "c40k-0.5b/adapter"
        adapter.mkdir(parents=True)
        (adapter / "adapter_config.json").write_text("{}")
        (adapter / "adapter_model.safetensors").write_bytes(b"weights")
        (adapter / "tokenizer.json").write_text("big")  # not needed: the base has the tokenizer
        (d / "t.jsonl").write_text('{"chosen": "a"}\n')
        (d / "v.jsonl").write_text('{"chosen": "b"}\n')
        out = d / "bundle"
        subprocess.run(
            [sys.executable, "-I", str(HERE / "kaggle_dpo_bundle.py"), str(out),
             f"b1=0.5b:{adapter}:{d / 't.jsonl'}:{d / 'v.jsonl'}:--beta 0.3 --lr 1e-5",
             f"b2=0.5b:{adapter}:{d / 't.jsonl'}:{d / 'v.jsonl'}"],
            check=True, capture_output=True, cwd=HERE,
        )
        spec = json.loads((out / "runs.json").read_text())
        first, second = spec["runs"]
        assert first["args"] == ["--dtype", "fp16", "--beta", "0.3", "--lr", "1e-5"]
        assert second["args"] == ["--dtype", "fp16"]
        assert first["base"]["repo"] == "Qwen/Qwen2.5-Coder-0.5B-Instruct"
        assert first["sft_sha256"] == hashlib.sha256(b"weights").hexdigest()
        assert (out / first["train"]).read_text() == '{"chosen": "a"}\n'
        assert (out / first["val"]).read_text() == '{"chosen": "b"}\n'
        sft = out / first["sft"]
        assert sorted(p.name for p in sft.iterdir()) == ["adapter_config.json", "adapter_model.safetensors"]
        for name in ("train.py", "train_dpo.py", "requirements.txt", "dataset-metadata.json"):
            assert (out / name).exists(), name
        assert "torch" not in (out / "requirements.txt").read_text()


def test_the_runs_per_gpu_reach_the_notebook_which_keeps_that_many_going() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        adapter = d / "a/adapter"
        adapter.mkdir(parents=True)
        for name in ("adapter_config.json", "adapter_model.safetensors"):
            (adapter / name).write_text("x")
        (d / "t.jsonl").write_text("{}\n")
        spec = f"r=0.5b:{adapter}:{d / 't.jsonl'}:{d / 't.jsonl'}"
        for flags, want in (([], 1), (["--per-gpu", "3"], 3)):
            out = d / f"bundle{want}"
            subprocess.run([sys.executable, "-I", str(HERE / "kaggle_dpo_bundle.py"), str(out), *flags, spec],
                           check=True, capture_output=True, cwd=HERE)
            assert json.loads((out / "runs.json").read_text())["per_gpu"] == want
    cells = json.loads((HERE / "kaggle_dpo.ipynb").read_text())["cells"]
    scheduler = next("".join(c["source"]) for c in cells if "pending, running" in "".join(c["source"]))
    assert "spec.get('per_gpu', 1)" in scheduler and "< per_gpu" in scheduler


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


def test_an_unknown_base_model_is_refused() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        refused = subprocess.run(
            [sys.executable, "-I", str(HERE / "kaggle_dpo_bundle.py"), str(d / "out"), f"x=7b:{d}:{d}:{d}"],
            capture_output=True, text=True, cwd=HERE,
        )
        assert refused.returncode != 0 and "no base model '7b'" in refused.stderr


def test_the_notebook_removes_kaggles_torchao_and_fails_when_a_run_does() -> None:
    """Kaggle's torchao 0.10 made peft 0.21 refuse every 16-bit layer, all six runs
    exited with 1, and the kernel still said COMPLETE."""
    cells = json.loads((HERE / "kaggle_dpo.ipynb").read_text())["cells"]
    code = ["".join(c["source"]) for c in cells if c["cell_type"] == "code"]
    install = next(c for c in code if "pip install" in c)
    assert install.index("pip uninstall -y -q torchao") < install.index("pip install -q -r")
    scheduler = next(c for c in code if "pending, running" in c)
    assert "assert not failed" in scheduler.splitlines()[-1]


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

"""Checks the bundle of the Kaggle DPO runs.

    python -I test_kaggle_dpo_bundle.py
"""

import hashlib
import json
import subprocess
import sys
import tempfile
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


def test_an_unknown_base_model_is_refused() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        refused = subprocess.run(
            [sys.executable, "-I", str(HERE / "kaggle_dpo_bundle.py"), str(d / "out"), f"x=7b:{d}:{d}:{d}"],
            capture_output=True, text=True, cwd=HERE,
        )
        assert refused.returncode != 0 and "no base model '7b'" in refused.stderr


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

"""Adapter (or nothing) + base model -> merged GGUF, quantized like the
upstream file smart-commit pins (Q4_K_M), with llama.cpp b11381.

    python -I export_gguf.py BASE_DIR OUT.gguf --llama-src DIR --llama-bin DIR
                             [--adapter DIR [--merged DIR]] [--quant Q4_K_M]

Without --adapter it re-quantizes the base model, the control that tells
our quantization apart from the fine-tune. --merged keeps the merged
safetensors the GGUF is made from, the files a fine-tune is published with.
"""

import argparse
import hashlib
import runpy
import subprocess
import sys
import tempfile
from pathlib import Path

import torch
from peft import PeftModel
from transformers import AutoModelForCausalLM, AutoTokenizer


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("base", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("--adapter", type=Path)
    parser.add_argument("--merged", type=Path, help="keep the merged model here")
    parser.add_argument("--llama-src", type=Path, required=True)
    parser.add_argument("--llama-bin", type=Path, required=True)
    parser.add_argument("--quant", default="Q4_K_M")
    args = parser.parse_args()
    if args.merged and not args.adapter:
        parser.error("--merged needs --adapter")

    with tempfile.TemporaryDirectory(dir=args.out.parent) as tmp:
        tmp = Path(tmp)
        source = args.base
        if args.adapter:
            model = AutoModelForCausalLM.from_pretrained(args.base, dtype=torch.bfloat16)
            model = PeftModel.from_pretrained(model, args.adapter).merge_and_unload()
            source = args.merged or tmp / "merged"
            model.save_pretrained(source)
            AutoTokenizer.from_pretrained(args.base).save_pretrained(source)
        bf16 = tmp / "model-bf16.gguf"
        sys.path[:0] = [str(args.llama_src), str(args.llama_src / "gguf-py")]
        sys.argv = ["convert_hf_to_gguf.py", str(source), "--outtype", "bf16",
                    "--outfile", str(bf16)]
        runpy.run_path(str(args.llama_src / "convert_hf_to_gguf.py"), run_name="__main__")
        subprocess.run(
            [str(args.llama_bin / "llama-quantize"), str(bf16), str(args.out), args.quant],
            check=True, capture_output=True,
            env={"LD_LIBRARY_PATH": str(args.llama_bin)},
        )
    digest = hashlib.sha256(args.out.read_bytes()).hexdigest()
    print(f"{digest}  {args.out}")


if __name__ == "__main__":
    main()

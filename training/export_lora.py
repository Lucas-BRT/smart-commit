"""PEFT adapter -> GGUF LoRA for `llama-server --lora`, with llama.cpp b11381.

Applied on the upstream GGUF (see llama-server-lora.sh), it measures the
fine-tune without the noise of re-quantizing a merged model.

    python -I export_lora.py BASE_DIR ADAPTER_DIR OUT.gguf --llama-src DIR
"""

import argparse
import hashlib
import runpy
import sys
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("base", type=Path)
    parser.add_argument("adapter", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("--llama-src", type=Path, required=True)
    args = parser.parse_args()

    sys.path[:0] = [str(args.llama_src), str(args.llama_src / "gguf-py")]
    sys.argv = ["convert_lora_to_gguf.py", "--base", str(args.base), "--outtype", "f16",
                "--outfile", str(args.out), str(args.adapter)]
    runpy.run_path(str(args.llama_src / "convert_lora_to_gguf.py"), run_name="__main__")
    print(f"{hashlib.sha256(args.out.read_bytes()).hexdigest()}  {args.out}")


if __name__ == "__main__":
    main()

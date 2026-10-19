"""LoRA / QLoRA fine-tune on `examples.jsonl` (from make_examples.py).

Each example is the model's own chat template over smart-commit's system
and user messages, then the target subject and the end-of-turn token; the
loss covers the reply only. Writes the adapter and `run.json` (everything
needed to repeat the run) to OUT_DIR.

    python -I train.py EXAMPLES.jsonl BASE_MODEL_DIR OUT_DIR [options]
"""

import argparse
import hashlib
import json
import platform
import random
import subprocess
import time
from importlib.metadata import version
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model, prepare_model_for_kbit_training
from transformers import (
    AutoModelForCausalLM,
    AutoTokenizer,
    BitsAndBytesConfig,
    Trainer,
    TrainingArguments,
)

TARGET_MODULES = ["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def encode(tok, example: dict, max_length: int) -> dict | None:
    messages = [
        {"role": "system", "content": example["system"]},
        {"role": "user", "content": example["user"]},
    ]
    prompt = tok.apply_chat_template(
        messages, add_generation_prompt=True, tokenize=True, return_dict=False
    )
    reply = tok(example["target"] + "<|im_end|>", add_special_tokens=False)["input_ids"]
    ids = prompt + reply
    if len(ids) > max_length:
        return None
    return {"input_ids": ids, "labels": [-100] * len(prompt) + reply}


class Collator:
    """Pads on the left, so every reply ends at the last position and the
    logits are only computed for the last `reply_width + 1` positions: the
    full 152k-token vocabulary over the prompt would not fit in 8 GB."""

    def __init__(self, pad_id: int):
        self.pad_id = pad_id

    def __call__(self, batch: list[dict]) -> dict:
        width = max(len(b["input_ids"]) for b in batch)
        reply = max(sum(1 for x in b["labels"] if x != -100) for b in batch)
        ids, labels, mask = [], [], []
        for b in batch:
            pad = width - len(b["input_ids"])
            ids.append([self.pad_id] * pad + b["input_ids"])
            labels.append([-100] * pad + b["labels"])
            mask.append([0] * pad + [1] * len(b["input_ids"]))
        mask = torch.tensor(mask)
        return {
            "input_ids": torch.tensor(ids),
            "labels": torch.tensor(labels)[:, -reply:],
            "attention_mask": mask,
            "position_ids": (mask.cumsum(1) - 1).clamp(min=0),
        }


class ReplyTrainer(Trainer):
    def compute_loss(self, model, inputs, return_outputs=False, num_items_in_batch=None):
        labels = inputs.pop("labels")
        reply = labels.shape[1]
        outputs = model(**inputs, logits_to_keep=reply + 1)
        logits = outputs.logits[:, :-1].float()
        loss = torch.nn.functional.cross_entropy(
            logits.reshape(-1, logits.shape[-1]), labels.reshape(-1).to(logits.device),
            ignore_index=-100, reduction="sum",
        )
        loss = loss / (num_items_in_batch if num_items_in_batch else (labels != -100).sum())
        return (loss, outputs) if return_outputs else loss


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("examples", type=Path)
    parser.add_argument("base", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("--quant", choices=["4bit", "16bit", "bf16"], default="4bit",
                        help="base weights: 4-bit (QLoRA) or 16-bit in --dtype (LoRA; "
                             "`bf16` is the old name of `16bit`)")
    parser.add_argument("--dtype", choices=["bf16", "fp16"], default="bf16",
                        help="compute precision; fp16 for GPUs without bf16 (T4, P100)")
    parser.add_argument("--resume", action="store_true",
                        help="continue from the last checkpoint in OUT_DIR, if any")
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--alpha", type=int, default=32)
    parser.add_argument("--dropout", type=float, default=0.05)
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--epochs", type=float, default=1.0)
    parser.add_argument("--batch", type=int, default=4)
    parser.add_argument("--accum", type=int, default=4)
    parser.add_argument("--max-length", type=int, default=1536)
    parser.add_argument("--limit", type=int, default=0, help="first N examples only")
    parser.add_argument("--val", type=Path,
                        help="examples from other repositories; their loss is logged ~10 times")
    parser.add_argument("--max-steps", type=int, default=-1)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    dtype = torch.bfloat16 if args.dtype == "bf16" else torch.float16
    random.seed(args.seed)
    torch.manual_seed(args.seed)
    tok = AutoTokenizer.from_pretrained(args.base)
    def load(path: Path, limit: int = 0) -> tuple[list[dict], int]:
        lines = path.read_text().splitlines()
        if limit:
            lines = lines[:limit]
        encoded = [encode(tok, json.loads(line), args.max_length) for line in lines]
        return [e for e in encoded if e is not None], sum(e is None for e in encoded)

    data, skipped = load(args.examples, args.limit)
    print(f"{len(data)} examples, {skipped} longer than {args.max_length} tokens skipped")
    val = load(args.val)[0] if args.val else None

    if args.quant == "4bit":
        model = AutoModelForCausalLM.from_pretrained(
            args.base,
            quantization_config=BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_quant_type="nf4",
                bnb_4bit_compute_dtype=dtype,
                bnb_4bit_use_double_quant=True,
            ),
            dtype=dtype,
            device_map={"": 0},
        )
        model = prepare_model_for_kbit_training(model, use_gradient_checkpointing=True)
    else:
        model = AutoModelForCausalLM.from_pretrained(
            args.base, dtype=dtype, device_map={"": 0}
        )
        model.gradient_checkpointing_enable()
        model.enable_input_require_grads()
    model = get_peft_model(
        model,
        LoraConfig(
            r=args.rank,
            lora_alpha=args.alpha,
            lora_dropout=args.dropout,
            target_modules=TARGET_MODULES,
            task_type="CAUSAL_LM",
        ),
    )
    model.print_trainable_parameters()

    training_args = TrainingArguments(
        output_dir=str(args.out / "checkpoints"),
        per_device_train_batch_size=args.batch,
        gradient_accumulation_steps=args.accum,
        num_train_epochs=args.epochs,
        max_steps=args.max_steps,
        learning_rate=args.lr,
        lr_scheduler_type="cosine",
        warmup_steps=0.03,
        weight_decay=0.0,
        # The adapters stay in fp32 (peft's default), so fp16 mixed
        # precision can unscale their gradients.
        bf16=args.dtype == "bf16",
        fp16=args.dtype == "fp16",
        logging_steps=10,
        save_strategy="steps",
        save_steps=200,
        save_total_limit=2,
        eval_strategy="steps" if val else "no",
        eval_steps=0.1,
        per_device_eval_batch_size=args.batch,
        prediction_loss_only=True,
        train_sampling_strategy="group_by_length",
        seed=args.seed,
        data_seed=args.seed,
        report_to=[],
        gradient_checkpointing=True,
        gradient_checkpointing_kwargs={"use_reentrant": False},
        dataloader_num_workers=0,
        remove_unused_columns=False,
    )
    trainer = ReplyTrainer(
        model=model,
        args=training_args,
        train_dataset=data,
        eval_dataset=val,
        data_collator=Collator(tok.pad_token_id),
    )
    started = time.time()
    checkpoints = args.out / "checkpoints"
    resume = args.resume and checkpoints.is_dir() and any(checkpoints.glob("checkpoint-*"))
    result = trainer.train(resume_from_checkpoint=True if resume else None)
    model.save_pretrained(args.out / "adapter")
    tok.save_pretrained(args.out / "adapter")

    gpu = subprocess.run(
        ["nvidia-smi", "--query-gpu=name,driver_version,memory.total", "--format=csv,noheader"],
        capture_output=True, text=True,
    ).stdout.strip()
    run = {
        "args": {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
        "examples_sha256": sha256(args.examples),
        "val_sha256": sha256(args.val) if args.val else None,
        "examples": len(data),
        "skipped_too_long": skipped,
        "base_config": json.loads((args.base / "config.json").read_text()).get("_name_or_path"),
        "base_safetensors_sha256": sha256(args.base / "model.safetensors"),
        "versions": {p: version(p) for p in
                     ("torch", "transformers", "peft", "bitsandbytes", "accelerate")},
        "cuda": torch.version.cuda,
        "gpu": gpu,
        "python": platform.python_version(),
        "train_seconds": round(time.time() - started),
        "resumed": resume,
        "train_loss": result.training_loss,
        "peak_vram_gib": round(torch.cuda.max_memory_allocated() / 2**30, 2),
        "log": trainer.state.log_history,
    }
    (args.out / "run.json").write_text(json.dumps(run, indent=2) + "\n")
    print(json.dumps({k: run[k] for k in ("train_seconds", "train_loss", "peak_vram_gib")}))


if __name__ == "__main__":
    main()

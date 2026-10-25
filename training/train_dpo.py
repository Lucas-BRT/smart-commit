"""DPO on the pairs of `sc-training pairs`, from a model already fine-tuned by train.py.

The base model gets the SFT adapter merged in (as export_gguf.py does), and a
new LoRA is trained on top: with it disabled the model is exactly the SFT one,
so it is also the reference of the loss, and no second copy is loaded. The loss
is the sigmoid DPO loss over the sum of the log-probabilities of the reply.
Writes the DPO adapter and `run.json` to OUT_DIR; `--merged DIR` also writes
the merged model, the BASE_DIR of export_gguf.py.

    python -I train_dpo.py PAIRS.jsonl BASE_MODEL_DIR SFT_ADAPTER_DIR OUT_DIR [options]
"""

import argparse
import json
import math
import platform
import random
import subprocess
import sys
import time
from importlib.metadata import version
from pathlib import Path

import torch
from peft import LoraConfig, PeftModel, get_peft_model
from transformers import AutoModelForCausalLM, AutoTokenizer

sys.path.insert(0, str(Path(__file__).parent))
from train import TARGET_MODULES, Collator, encode, sha256  # noqa: E402


def encode_pair(tok, pair: dict, max_length: int) -> dict | None:
    """Chosen and rejected over the same prompt; None if either is too long."""
    chosen, rejected = (
        encode(tok, {"system": pair["system"], "user": pair["user"], "target": pair[side]}, max_length)
        for side in ("chosen", "rejected")
    )
    return None if chosen is None or rejected is None else {"chosen": chosen, "rejected": rejected}


class PairCollator:
    """The chosen replies of the batch, then the rejected ones, as one batch of
    `Collator`: the logits are only computed for the replies."""

    def __init__(self, pad_id: int):
        self.collator = Collator(pad_id)

    def __call__(self, pairs: list[dict]) -> dict:
        return self.collator([p["chosen"] for p in pairs] + [p["rejected"] for p in pairs])


def reply_logps(model, batch: dict) -> torch.Tensor:
    """The sum of the log-probabilities of the reply of each sequence."""
    labels = batch["labels"]
    inputs = {k: v for k, v in batch.items() if k != "labels"}
    logits = model(**inputs, logits_to_keep=labels.shape[1] + 1).logits[:, :-1].float()
    labels = labels.to(logits.device)
    logps = torch.log_softmax(logits, dim=-1).gather(-1, labels.clamp(min=0).unsqueeze(-1)).squeeze(-1)
    return (logps * (labels != -100)).sum(-1)


def dpo_loss(policy: torch.Tensor, reference: torch.Tensor, beta: float) -> tuple[torch.Tensor, dict]:
    """`policy` and `reference` hold the chosen sequences, then the rejected ones."""
    half = policy.shape[0] // 2
    margin = beta * ((policy[:half] - reference[:half]) - (policy[half:] - reference[half:]))
    return -torch.nn.functional.logsigmoid(margin).mean(), {
        "accuracy": (margin > 0).float().mean().item(),
        "margin": margin.mean().item(),
    }


def learning_rate(step: int, steps: int, peak: float, warmup: float = 0.03) -> float:
    warmup_steps = max(1, math.ceil(warmup * steps))
    if step < warmup_steps:
        return peak * (step + 1) / warmup_steps
    return peak * 0.5 * (1 + math.cos(math.pi * (step - warmup_steps) / max(1, steps - warmup_steps)))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("pairs", type=Path)
    parser.add_argument("base", type=Path)
    parser.add_argument("sft", type=Path, help="the adapter train.py wrote")
    parser.add_argument("out", type=Path)
    parser.add_argument("--merged", type=Path, help="also write the merged model here")
    parser.add_argument("--dtype", choices=["bf16", "fp16"], default="bf16",
                        help="weights and compute; fp16 for GPUs without bf16 (T4, P100)")
    parser.add_argument("--beta", type=float, default=0.1)
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--alpha", type=int, default=32)
    parser.add_argument("--dropout", type=float, default=0.05)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--epochs", type=int, default=1)
    parser.add_argument("--batch", type=int, default=2, help="pairs per forward pass")
    parser.add_argument("--accum", type=int, default=8)
    parser.add_argument("--max-length", type=int, default=1536)
    parser.add_argument("--limit", type=int, default=0, help="first N pairs only")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    dtype = torch.bfloat16 if args.dtype == "bf16" else torch.float16
    random.seed(args.seed)
    torch.manual_seed(args.seed)
    tok = AutoTokenizer.from_pretrained(args.base)
    lines = args.pairs.read_text().splitlines()
    if args.limit:
        lines = lines[: args.limit]
    encoded = [encode_pair(tok, json.loads(line), args.max_length) for line in lines]
    data = [e for e in encoded if e is not None]
    skipped = len(encoded) - len(data)
    print(f"{len(data)} pairs, {skipped} longer than {args.max_length} tokens skipped")

    model = AutoModelForCausalLM.from_pretrained(args.base, dtype=dtype, device_map={"": 0})
    model = PeftModel.from_pretrained(model, args.sft).merge_and_unload()
    model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})
    model.enable_input_require_grads()
    model = get_peft_model(model, LoraConfig(
        r=args.rank, lora_alpha=args.alpha, lora_dropout=args.dropout,
        target_modules=TARGET_MODULES, task_type="CAUSAL_LM",
    ))
    model.print_trainable_parameters()
    model.train()

    per_step = args.batch * args.accum
    steps = args.epochs * (len(data) // per_step)
    if steps == 0:
        parser.error(f"{len(data)} pairs are fewer than one step of {per_step}")
    params = [p for p in model.parameters() if p.requires_grad]
    optimizer = torch.optim.AdamW(params, lr=args.lr, weight_decay=0.0)
    # fp16 gradients of the fp32 adapters need scaling; bf16 does not.
    scaler = torch.amp.GradScaler(enabled=args.dtype == "fp16")
    collate = PairCollator(tok.pad_token_id)
    order = random.Random(args.seed)
    log, started = [], time.time()
    for epoch in range(args.epochs):
        shuffled = order.sample(data, len(data))
        for step in range(len(data) // per_step):
            number = epoch * (len(data) // per_step) + step
            for group in optimizer.param_groups:
                group["lr"] = learning_rate(number, steps, args.lr)
            totals = {"loss": 0.0, "accuracy": 0.0, "margin": 0.0}
            for micro in range(args.accum):
                at = (step * args.accum + micro) * args.batch
                batch = {k: v.to("cuda") for k, v in collate(shuffled[at : at + args.batch]).items()}
                with torch.no_grad(), model.disable_adapter(), torch.autocast("cuda", dtype=dtype):
                    reference = reply_logps(model, batch)
                with torch.autocast("cuda", dtype=dtype):
                    policy = reply_logps(model, batch)
                loss, stats = dpo_loss(policy, reference, args.beta)
                scaler.scale(loss / args.accum).backward()
                totals["loss"] += loss.item() / args.accum
                for k, v in stats.items():
                    totals[k] += v / args.accum
            scaler.unscale_(optimizer)
            torch.nn.utils.clip_grad_norm_(params, 1.0)
            scaler.step(optimizer)
            scaler.update()
            optimizer.zero_grad(set_to_none=True)
            log.append({"step": number + 1, **totals, "lr": optimizer.param_groups[0]["lr"]})
            if (number + 1) % 5 == 0 or number + 1 == steps:
                print(json.dumps(log[-1]), flush=True)

    model.save_pretrained(args.out / "adapter")
    tok.save_pretrained(args.out / "adapter")
    if args.merged:
        model.merge_and_unload().save_pretrained(args.merged)
        tok.save_pretrained(args.merged)

    gpu = subprocess.run(
        ["nvidia-smi", "--query-gpu=name,driver_version,memory.total", "--format=csv,noheader"],
        capture_output=True, text=True,
    ).stdout.strip()
    run = {
        "args": {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
        "pairs_sha256": sha256(args.pairs),
        "sft_adapter_sha256": sha256(args.sft / "adapter_model.safetensors"),
        "pairs": len(data),
        "skipped_too_long": skipped,
        "steps": steps,
        "base_safetensors_sha256": sha256(args.base / "model.safetensors"),
        "versions": {p: version(p) for p in ("torch", "transformers", "peft", "accelerate")},
        "cuda": torch.version.cuda,
        "gpu": gpu,
        "python": platform.python_version(),
        "train_seconds": round(time.time() - started),
        "peak_vram_gib": round(torch.cuda.max_memory_allocated() / 2**30, 2),
        "log": log,
    }
    (args.out / "run.json").write_text(json.dumps(run, indent=2) + "\n")
    print(json.dumps({k: run[k] for k in ("train_seconds", "peak_vram_gib")}), json.dumps(log[-1]))


if __name__ == "__main__":
    main()

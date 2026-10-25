"""Checks the pieces of `train_dpo` on the CPU, with a tiny random model.

    python -I test_train_dpo.py
"""

import math
import sys
from pathlib import Path

import torch
from transformers import Qwen2Config, Qwen2ForCausalLM

sys.path.insert(0, str(Path(__file__).parent))
from train_dpo import PairCollator, dpo_loss, encode_pair, learning_rate, reply_logps  # noqa: E402


class Tok:
    """One token per character; the template is `<system>|<user>|`."""

    def apply_chat_template(self, messages, add_generation_prompt, tokenize, return_dict):
        return [ord(c) for c in "|".join(m["content"] for m in messages) + "|"]

    def __call__(self, text, add_special_tokens):
        return {"input_ids": [ord(c) for c in text]}


def pair(chosen: str = "fix: a", rejected: str = "chore: b") -> dict:
    return {"system": "S", "user": "u", "chosen": chosen, "rejected": rejected}


def test_the_loss_prefers_the_chosen_reply() -> None:
    reference = torch.tensor([-10.0, -20.0, -12.0, -18.0])  # chosen x2, rejected x2
    # Untrained: the policy is the reference, the margin is 0 and the loss log 2.
    loss, stats = dpo_loss(reference.clone(), reference, 0.1)
    assert math.isclose(loss.item(), math.log(2), rel_tol=1e-6)
    assert stats["accuracy"] == 0.0 and stats["margin"] == 0.0
    # The chosen replies up by 5 and the rejected down by 5: margin beta * 10.
    policy = reference + torch.tensor([5.0, 5.0, -5.0, -5.0])
    loss, stats = dpo_loss(policy, reference, 0.1)
    assert math.isclose(stats["margin"], 1.0, rel_tol=1e-6)
    assert math.isclose(loss.item(), -math.log(1 / (1 + math.exp(-1.0))), rel_tol=1e-6)
    assert stats["accuracy"] == 1.0
    # The other way round is worse than untrained.
    worse, stats = dpo_loss(reference - torch.tensor([5.0, 5.0, -5.0, -5.0]), reference, 0.1)
    assert worse.item() > math.log(2) and stats["accuracy"] == 0.0


def test_the_learning_rate_warms_up_then_decays_to_zero() -> None:
    rates = [learning_rate(s, 100, 1.0) for s in range(100)]
    assert rates[0] < rates[2] == 1.0  # warmup of 3 steps
    assert all(a >= b for a, b in zip(rates[2:], rates[3:]))
    assert rates[-1] < 1e-3
    assert learning_rate(0, 1, 1.0) == 1.0


def test_a_pair_is_the_same_prompt_with_each_reply() -> None:
    encoded = encode_pair(Tok(), pair(), 100)
    prompt = [ord(c) for c in "S|u|"]
    chosen, rejected = encoded["chosen"], encoded["rejected"]
    assert chosen["input_ids"][: len(prompt)] == rejected["input_ids"][: len(prompt)] == prompt
    assert chosen["labels"][: len(prompt)] == [-100] * len(prompt)
    assert chosen["labels"][len(prompt):] == [ord(c) for c in "fix: a<|im_end|>"]
    assert rejected["labels"][len(prompt):] == [ord(c) for c in "chore: b<|im_end|>"]
    # Too long on either side drops the pair.
    assert encode_pair(Tok(), pair(chosen="x" * 200), 100) is None
    assert encode_pair(Tok(), pair(rejected="x" * 200), 100) is None


def test_the_batch_holds_the_chosen_replies_then_the_rejected_ones() -> None:
    pairs = [encode_pair(Tok(), pair("fix: a", "chore: b"), 100), encode_pair(Tok(), pair("feat: c", "docs: d"), 100)]
    batch = PairCollator(0)(pairs)
    assert batch["input_ids"].shape[0] == 4
    replies = [[t for t in row.tolist() if t != -100] for row in batch["labels"]]
    assert [len(r) for r in replies] == [len(pairs[0]["chosen"]["labels"]) - 4,
                                          len(pairs[1]["chosen"]["labels"]) - 4,
                                          len(pairs[0]["rejected"]["labels"]) - 4,
                                          len(pairs[1]["rejected"]["labels"]) - 4]
    assert batch["labels"].shape[1] == max(len(r) for r in replies)


def test_reply_logps_is_the_log_probability_of_the_reply_given_the_prompt() -> None:
    torch.manual_seed(0)
    config = Qwen2Config(vocab_size=300, hidden_size=16, intermediate_size=32, num_hidden_layers=2,
                         num_attention_heads=2, num_key_value_heads=2)
    model = Qwen2ForCausalLM(config).eval()
    pairs = [encode_pair(Tok(), pair("fix: a", "chore: bb"), 100), encode_pair(Tok(), pair("feat: c", "docs: d"), 100)]
    batch = PairCollator(0)(pairs)
    with torch.no_grad():
        got = reply_logps(model, batch)
        # By hand, one sequence at a time and without padding or `logits_to_keep`.
        want = []
        for side in ("chosen", "rejected"):
            for p in pairs:
                ids, labels = p[side]["input_ids"], p[side]["labels"]
                logps = torch.log_softmax(model(torch.tensor([ids])).logits[0].float(), -1)
                want.append(sum(logps[i - 1, ids[i]].item() for i, label in enumerate(labels) if label != -100))
    assert got.shape == (4,)
    assert torch.allclose(got, torch.tensor(want), atol=1e-4), (got, want)


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

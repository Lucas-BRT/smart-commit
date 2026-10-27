"""Checks the pieces of `train_dpo` on the CPU, with a tiny random model.

    python -I test_train_dpo.py
"""

import contextlib
import json
import math
import sys
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model
from transformers import Qwen2Config, Qwen2ForCausalLM

sys.path.insert(0, str(Path(__file__).parent))
from train import split_lines, weights_sha256  # noqa: E402
from train_dpo import PairCollator, dpo_loss, encode_pair, evaluate, learning_rate, pair_loss, reply_logps  # noqa: E402


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


def tiny_policy() -> torch.nn.Module:
    """A random model with a fresh LoRA, as train_dpo makes: the adapter does
    nothing yet, so the model is its own reference."""
    torch.manual_seed(0)
    config = Qwen2Config(vocab_size=300, hidden_size=16, intermediate_size=32, num_hidden_layers=2,
                         num_attention_heads=2, num_key_value_heads=2)
    lora = LoraConfig(r=4, lora_alpha=8, lora_dropout=0.0, target_modules=["q_proj", "v_proj"], task_type="CAUSAL_LM")
    return get_peft_model(Qwen2ForCausalLM(config), lora)


def some_pairs() -> list[dict]:
    return [
        encode_pair(Tok(), pair("fix: a", "chore: bb"), 100),
        encode_pair(Tok(), pair("feat: c", "docs: d"), 100),
        encode_pair(Tok(), pair("fix: e", "feat: f"), 100),
        encode_pair(Tok(), pair("docs: g", "fix: h"), 100),
    ]


def test_rewards_are_beta_times_the_gain_over_the_reference() -> None:
    reference = torch.tensor([-10.0, -20.0, -12.0, -18.0])
    policy = reference + torch.tensor([4.0, 2.0, -6.0, -2.0])
    _, stats = dpo_loss(policy, reference, 0.5)
    assert math.isclose(stats["chosen_reward"], 1.5) and math.isclose(stats["rejected_reward"], -2.0)
    assert math.isclose(stats["margin"], 3.5)


def test_an_untrained_adapter_leaves_the_model_its_own_reference() -> None:
    model = tiny_policy().eval()
    batch = PairCollator(0)(some_pairs())
    loss, stats = pair_loss(model, batch, 0.1, 0.0, contextlib.nullcontext)
    assert math.isclose(loss.item(), math.log(2), rel_tol=1e-6)
    assert stats["margin"] == 0.0 and stats["chosen_reward"] == 0.0 and stats["rejected_reward"] == 0.0
    # The chosen reply's own loss, per token, when asked for.
    with_sft, with_stats = pair_loss(model, batch, 0.1, 0.5, contextlib.nullcontext)
    assert with_stats["nll"] > 0 and math.isclose(with_sft.item(), math.log(2) + 0.5 * with_stats["nll"], rel_tol=1e-5)


def test_a_few_steps_separate_the_chosen_reply_from_the_rejected_one() -> None:
    model = tiny_policy()
    model.train()
    pairs = some_pairs()
    batch = PairCollator(0)(pairs)
    params = [p for p in model.parameters() if p.requires_grad]
    optimizer = torch.optim.AdamW(params, lr=5e-2)
    first = None
    for _ in range(25):
        loss, stats = pair_loss(model, batch, 0.1, 0.0, contextlib.nullcontext)
        first = first or stats
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    assert stats["loss"] < first["loss"] - 0.05 and stats["accuracy"] == 1.0
    assert stats["chosen_reward"] > 0 > stats["rejected_reward"]


def test_the_validation_pairs_are_scored_without_training_on_them() -> None:
    model = tiny_policy()
    model.train()
    before = [p.detach().clone() for p in model.parameters()]
    args = type("Args", (), {"batch": 3, "beta": 0.1, "sft_weight": 0.0})()
    got = evaluate(model, some_pairs(), PairCollator(0), args, contextlib.nullcontext, "cpu")
    assert {"val_loss", "val_accuracy", "val_margin", "val_chosen_reward"} <= set(got)
    assert math.isclose(got["val_loss"], math.log(2), rel_tol=1e-6)
    assert model.training and all(torch.equal(a, b) for a, b in zip(before, model.parameters()))


def test_the_lines_of_a_jsonl_file_are_cut_at_newlines_only() -> None:
    """str.splitlines() also cuts at U+2028, U+0085 and others, which JSON allows in a string."""
    row = json.dumps({"diff": "a\u2028b\u0085c\x0cd"}, ensure_ascii=False)
    assert len(row.splitlines()) > 1  # the trap
    assert split_lines(row + "\n" + row + "\n") == [row, row]
    assert split_lines("") == [] and split_lines("{}") == ["{}"]


def test_the_weights_of_a_base_are_hashed_whole_or_by_shard_and_a_base_without_fails() -> None:
    import hashlib
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "whole").mkdir()
        (d / "whole/model.safetensors").write_bytes(b"w")
        assert weights_sha256(d / "whole") == hashlib.sha256(b"w").hexdigest()
        (d / "sharded").mkdir()
        (d / "sharded/model-00001-of-00002.safetensors").write_bytes(b"a")
        (d / "sharded/model-00002-of-00002.safetensors").write_bytes(b"b")
        assert weights_sha256(d / "sharded") == {
            "model-00001-of-00002.safetensors": hashlib.sha256(b"a").hexdigest(),
            "model-00002-of-00002.safetensors": hashlib.sha256(b"b").hexdigest(),
        }
        (d / "none").mkdir()
        try:
            weights_sha256(d / "none")
        except SystemExit as e:
            assert "no .safetensors" in str(e)
        else:
            raise AssertionError("a base without weights was hashed")


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

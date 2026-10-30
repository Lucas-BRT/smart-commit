"""Checks the pure parts of `cascade_bench` (no server needed).

    python -I test_cascade_bench.py
"""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from cascade_bench import body, cold, confidence, designs, percentile, reply_type, stats  # noqa: E402


def reply(*top: tuple[str, float]) -> dict:
    return {"choices": [{"message": {"content": "x"}, "logprobs": {"content": [
        {"token": top[0][0], "logprob": math.log(top[0][1]),
         "top_logprobs": [{"token": t, "logprob": math.log(p)} for t, p in top]}]}}]}


def test_the_request_is_the_products() -> None:
    prompt = {"system": "S", "user": "U"}
    r = body(prompt)
    assert r["messages"] == [{"role": "system", "content": "S"}, {"role": "user", "content": "U"}]
    assert (r["max_tokens"], r["temperature"], r["top_k"], r["seed"], r["cache_prompt"]) == (128, 0, 1, 0, False)
    assert "logprobs" not in r
    p = body(prompt, max_tokens=1, logprobs=True)
    assert p["max_tokens"] == 1 and p["logprobs"] is True and p["top_logprobs"] == 12


def test_the_confidence_is_the_likeliest_type_among_the_types() -> None:
    kind, conf = confidence(reply(("fix", 0.5), ("feat", 0.3), ("The", 0.2)))     # `The` is no type: 0.5 of 0.8
    assert kind == "fix" and abs(conf - 0.625) < 1e-9
    assert confidence(reply(("The", 1.0))) == (None, 0.0)
    assert confidence({"choices": [{"message": {"content": "x"}}]}) == (None, 0.0)


def test_the_type_of_a_reply_line() -> None:
    assert reply_type("fix(core)!: x") == "fix" and reply_type("Tests: y") == "test"
    assert reply_type("update the thing") is None and reply_type("") is None and reply_type("add a thing: and") is None


def test_percentiles_and_stats() -> None:
    assert percentile([5, 1, 3, 2, 4], 0.5) == 3 and percentile([5, 1, 3, 2, 4], 0.9) == 5
    assert percentile([7], 0.9) == 7
    s = stats([1.0, 2.0, 3.0, 4.0])
    assert s == {"mean": 2.5, "p50": 2.0, "p90": 4.0}


ROWS = [
    {"probe": 0.4, "small_lp": 1.0, "small": 0.9, "big": 2.0, "conf": 0.9},     # sure
    {"probe": 0.5, "small_lp": 1.2, "small": 1.1, "big": 2.4, "conf": 0.3},     # not sure
    {"probe": 0.4, "small_lp": 1.0, "small": 1.0, "big": 2.2, "conf": 0.5},     # exactly at tau: sure
    {"probe": 0.5, "small_lp": 1.4, "small": 1.3, "big": 2.6, "conf": 0.4},     # not sure
]


def test_each_design_adds_what_it_runs() -> None:
    d = designs(ROWS, 0.5)
    assert d["escalated"] == 0.5
    assert d["big alone"] == [2.0, 2.4, 2.2, 2.6] and d["small alone"] == [0.9, 1.1, 1.0, 1.3]
    # single pass: the small reply with probabilities, and the big one when not sure
    assert [round(x, 3) for x in d["single pass"]] == [1.0, 3.6, 1.0, 4.0]
    # probe, then the small reply when sure (a second reading of the prompt), the big one otherwise
    assert [round(x, 3) for x in d["probe, then reply"]] == [1.3, 2.9, 1.4, 3.1]
    assert designs(ROWS, 0.0)["escalated"] == 0.0 and designs(ROWS, 1.01)["escalated"] == 1.0


def test_when_nothing_is_kept_loaded_each_message_pays_its_loads() -> None:
    c = cold(ROWS, 0.5, load_small=1.0, load_big=2.0)
    assert [round(x, 3) for x in c["big alone"]] == [4.0, 4.4, 4.2, 4.6]
    # the small load for all, the big one for the two escalations
    assert [round(x, 3) for x in c["single pass"]] == [2.0, 1.0 + 3.6 + 2.0, 2.0, 1.0 + 4.0 + 2.0]


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

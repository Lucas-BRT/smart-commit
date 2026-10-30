"""A blind pairwise judge of the quality of commit messages (H18).

`word_f1` compares a message with what the author wrote, and authors write "fix typo". This asks annotators who do not
know where a message comes from which of two is the better commit message for the same change, and how good each is.

- `build` takes the commits of an audit set (`audit_types.py`) and, for each, compares the focus model's message with the
  author's and with other models' (identical messages are a tie and are not asked). Each of two passes, A and B,
  has its own random side for each message, so the side and the pass can be told apart from the quality.
- `merge` reads the annotators' answers and writes `judgments.json`, with each outcome from the focus model's side.
- `report` shows wins, ties and losses against each, the scores, the messages that invent, the positional bias and the
  agreement of the two passes.

    python -I pair_judge.py build AUDIT_DIR SET_DIR OUT_DIR FOCUS=TRACES.jsonl OTHER=TRACES.jsonl... [--seed 29]
    python -I pair_judge.py merge OUT_DIR
    python -I pair_judge.py report OUT_DIR/judgments.json

`AUDIT_DIR/sample.json` lists the commits (`id`, `line`, `real_subject`); the first model given is the focus, the
author is added as `author`. Standard library only.
"""

import collections
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from audit_types import batches_for, render_case  # noqa: E402
from common import bootstrap, bounds  # noqa: E402

PASSES = ("A", "B")
BATCHES = 4
BOOTSTRAPS = 2000
SEED = 29


def norm(message: str) -> str:
    return " ".join(message.lower().split())


def subjects(traces: Path, lines: list[int]) -> dict[int, str]:
    wanted = set(lines)
    out = {}
    for index, line in enumerate(open(traces)):
        if index in wanted:
            out[index] = json.loads(line)["generation"]["message"]["subject"]
    return out


def side(seed: int, pass_name: str, comparison_id: str) -> str:
    """Which side the focus message takes in this pass: `X` or `Y`."""
    return "X" if random.Random(f"{seed}-{pass_name}-{comparison_id}").random() < 0.5 else "Y"


def comparisons_for(cid: str, texts: dict[str, str], focus: str) -> list[dict]:
    """The comparisons of a case, numbered by the position of the other candidate; the ones that are identical are
    ties no one is asked."""
    out = []
    others = [name for name in texts if name != focus]
    for number, other in enumerate(others, 1):
        out.append({
            "id": f"{cid}-{number}", "other": other, "focus_text": texts[focus], "other_text": texts[other],
            "auto_tie": norm(texts[focus]) == norm(texts[other]),
        })
    return out


def render_comparisons(case: dict, seed: int, pass_name: str) -> str:
    lines = ["", "### Messages to judge", ""]
    for c in case["comparisons"]:
        if c["auto_tie"]:
            continue
        focus_side = side(seed, pass_name, c["id"])
        x, y = (c["focus_text"], c["other_text"]) if focus_side == "X" else (c["other_text"], c["focus_text"])
        lines += [f"Comparison {c['id']}:", f"- X: {x}", f"- Y: {y}", ""]
    return "\n".join(lines)


def build(audit_dir: Path, set_dir: Path, out_dir: Path, models: list[tuple[str, Path]], seed: int) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    key = json.loads((audit_dir / "sample.json").read_text())
    inputs = [json.loads(line) for line in open(set_dir / "inputs.jsonl")]
    lines = [k["line"] for k in key]
    focus = models[0][0]
    by_model = {name: subjects(path, lines) for name, path in models}
    cases = []
    for k in key:
        texts = {"author": k["real_subject"], **{name: by_model[name][k["line"]] for name, _ in models}}
        cases.append({"id": k["id"], "line": k["line"], "comparisons": comparisons_for(k["id"], texts, focus)})
    (out_dir / "pairs.json").write_text(json.dumps({"seed": seed, "focus": focus, "cases": cases}, indent=1))
    ids = [c["id"] for c in cases]
    by_id = {c["id"]: c for c in cases}
    for pass_name in PASSES:
        for number, batch in enumerate(batches_for(ids, f"pair-{pass_name}", seed, BATCHES), 1):
            blocks = []
            for cid in batch:
                case = by_id[cid]
                if all(c["auto_tie"] for c in case["comparisons"]):
                    continue
                blocks.append(render_case(cid, inputs[case["line"]]) + "\n" + render_comparisons(case, seed, pass_name))
            asked = [c["id"] for cid in batch for c in by_id[cid]["comparisons"] if not c["auto_tie"]]
            (out_dir / f"P{pass_name}-{number}.md").write_text("\n\n".join(blocks) + "\n")
            (out_dir / f"P{pass_name}-{number}.ids").write_text("\n".join(asked) + "\n")


def check(answer: dict, asked: set[str]) -> list[str]:
    problems = []
    if answer.get("id") not in asked:
        problems.append(f"unknown comparison {answer.get('id')!r}")
    if answer.get("winner") not in ("X", "Y", "tie"):
        problems.append(f"winner {answer.get('winner')!r}")
    for key in ("score_x", "score_y"):
        if not isinstance(answer.get(key), int) or not 1 <= answer[key] <= 5:
            problems.append(f"{key} {answer.get(key)!r} is not an integer from 1 to 5")
    for key in ("invented_x", "invented_y"):
        if not isinstance(answer.get(key), bool):
            problems.append(f"{key} {answer.get(key)!r} is not true or false")
    return problems


def outcome(answer: dict, focus_side: str) -> dict:
    """The answer from the focus model's side."""
    other_side = "Y" if focus_side == "X" else "X"
    winner = answer["winner"]
    result = "tie" if winner == "tie" else ("win" if winner == focus_side else "loss")
    return {
        "outcome": result,
        "score_focus": answer[f"score_{focus_side.lower()}"], "score_other": answer[f"score_{other_side.lower()}"],
        "invented_focus": answer[f"invented_{focus_side.lower()}"], "invented_other": answer[f"invented_{other_side.lower()}"],
        "focus_side": focus_side,
    }


def merge(out_dir: Path) -> None:
    pairs = json.loads((out_dir / "pairs.json").read_text())
    seed = pairs["seed"]
    info = {c["id"]: (c, case["id"]) for case in pairs["cases"] for c in case["comparisons"]}
    judgments = []
    for pass_name in PASSES:
        expected = {c["id"] for case in pairs["cases"] for c in case["comparisons"] if not c["auto_tie"]}
        seen: set[str] = set()
        for path in sorted(out_dir.glob(f"P{pass_name}-*.json")):
            for answer in json.loads(path.read_text()):
                problems = check(answer, expected)
                if problems:
                    sys.exit(f"{path.name}: {answer.get('id')}: {'; '.join(problems)}")
                if answer["id"] in seen:
                    sys.exit(f"pass {pass_name}: {answer['id']} judged twice")
                seen.add(answer["id"])
                c, case_id = info[answer["id"]]
                judgments.append({
                    "id": answer["id"], "case": case_id, "pass": pass_name, "other": c["other"],
                    **outcome(answer, side(seed, pass_name, answer["id"])), "reason": str(answer.get("reason", ""))[:240],
                })
        missing = expected - seen
        if missing:
            sys.exit(f"pass {pass_name}: {len(missing)} comparisons not judged, e.g. {sorted(missing)[:3]}")
    for case in pairs["cases"]:
        for c in case["comparisons"]:
            if c["auto_tie"]:
                for pass_name in PASSES:
                    judgments.append({"id": c["id"], "case": case["id"], "pass": pass_name, "other": c["other"],
                                      "outcome": "tie", "auto_tie": True})
    (out_dir / "judgments.json").write_text(json.dumps(judgments, indent=1))
    print(f"{len(judgments)} judgments merged into {out_dir / 'judgments.json'}")


def net(outcomes: list[str]) -> float:
    """Wins minus losses, over all the comparisons (ties count as neither)."""
    return (outcomes.count("win") - outcomes.count("loss")) / len(outcomes) if outcomes else 0.0


def interval(outcomes: list[str]) -> tuple[float, float, float]:
    values = bootstrap(outcomes, net, SEED, BOOTSTRAPS) if outcomes else [0.0]
    return (net(outcomes), *bounds(values))


def agreement(judgments: list[dict]) -> dict:
    """How the two passes agree on the same comparisons (ties in either pass included)."""
    by = collections.defaultdict(dict)
    for j in judgments:
        if not j.get("auto_tie"):
            by[j["id"]][j["pass"]] = j["outcome"]
    both = [v for v in by.values() if len(v) == 2]
    decided = [v for v in both if "tie" not in v.values()]
    return {
        "compared": len(both), "same": sum(v["A"] == v["B"] for v in both) / len(both) if both else 0.0,
        "decided_both": len(decided),
        "same_when_decided": sum(v["A"] == v["B"] for v in decided) / len(decided) if decided else 0.0,
    }


def report(path: Path) -> None:
    js = json.loads(path.read_text())
    real = [j for j in js if not j.get("auto_tie")]
    print(f"{len(js) // 2} comparisons x 2 passes; {len(js) - len(real)} are the same message on both sides (ties)")
    for other in sorted({j["other"] for j in js}):
        mine = [j for j in js if j["other"] == other]
        o = [j["outcome"] for j in mine]
        n, lo, hi = interval(o)
        asked = [j for j in mine if not j.get("auto_tie")]
        scores = [(j["score_focus"], j["score_other"]) for j in asked]
        print(f"focus against {other}: wins {o.count('win')}, ties {o.count('tie')}, losses {o.count('loss')}  "
              f"(net {n:+.2f} [{lo:+.2f}, {hi:+.2f}])")
        if scores:
            print(f"   score of the focus {sum(a for a, _ in scores) / len(scores):.2f}, of {other} "
                  f"{sum(b for _, b in scores) / len(scores):.2f}; invents: focus "
                  f"{sum(j['invented_focus'] for j in asked) / len(asked):.2f}, {other} "
                  f"{sum(j['invented_other'] for j in asked) / len(asked):.2f}")
        for pass_name in PASSES:
            op = [j["outcome"] for j in mine if j["pass"] == pass_name]
            print(f"   pass {pass_name}: {op.count('win')} / {op.count('tie')} / {op.count('loss')}")
    sides = collections.Counter()
    for j in real:
        if j["outcome"] != "tie":
            sides["focus on X" if j["focus_side"] == "X" else "focus on Y"] += 1
    a = agreement(js)
    wins_on_x = sum(1 for j in real if j["outcome"] != "tie" and ((j["focus_side"] == "X") == (j["outcome"] == "win")))
    decided = sum(sides.values())
    print(f"position: X won {wins_on_x} of {decided} decided comparisons ({wins_on_x / decided:.2f})" if decided else "")
    print(f"the two passes: same outcome in {a['same']:.2f} of {a['compared']} comparisons; {a['same_when_decided']:.2f} "
          f"of the {a['decided_both']} that both decided")


def main() -> None:
    args = sys.argv[1:]
    if args[:1] == ["build"]:
        seed = int(args[args.index("--seed") + 1]) if "--seed" in args else SEED
        models = [(a.split("=", 1)[0], Path(a.split("=", 1)[1])) for a in args[4:] if "=" in a]
        build(Path(args[1]), Path(args[2]), Path(args[3]), models, seed)
    elif args[:1] == ["merge"]:
        merge(Path(args[1]))
    elif args[:1] == ["report"]:
        report(Path(args[1]))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()

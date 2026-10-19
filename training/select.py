"""Deterministic subsets of `build_inputs.py` output.

Commits are ranked by the SHA-256 of their hash, so the choice does not
depend on the dataset's order; at most `--per-repo` per repository keeps one
large repository from dominating.

    python -I select.py SRC_DIR OUT_DIR --conventional N --free N [--per-repo K]
                        [--min-history H] [--max-type-share F]
"""

import argparse
import collections
import hashlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from build_inputs import keep_target  # noqa: E402
from score import evidence, unsupported  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("src", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("--conventional", type=int, required=True,
                        help="commits with a Conventional subject from mostly Conventional repos")
    parser.add_argument("--free", type=int, required=True,
                        help="commits from repos that are mostly not Conventional")
    parser.add_argument("--per-repo", type=int, default=40)
    parser.add_argument("--min-history", type=int, default=10)
    parser.add_argument("--supported-only", action="store_true",
                        help="drop commits whose real subject names a version, number or "
                             "identifier absent from the diff and the history")
    parser.add_argument("--max-type-share", type=float, default=1.0,
                        help="at most this share of the conventional commits per type")
    args = parser.parse_args()

    meta = [json.loads(line) for line in (args.src / "meta.jsonl").read_text().splitlines()]
    # inputs.jsonl can be larger than memory: index line offsets, read lines on demand.
    offsets = []
    with open(args.src / "inputs.jsonl", "rb") as f:
        position = 0
        for line in f:
            offsets.append(position)
            position += len(line)
    assert len(offsets) == len(meta)
    inputs_file = open(args.src / "inputs.jsonl", "rb")

    def input_line(i: int) -> str:
        inputs_file.seek(offsets[i])
        return inputs_file.readline().decode().rstrip("\n")
    share = collections.defaultdict(list)
    for m in meta:
        share[m["repo"]].append(m["type"] is not None)
    conventional_repo = {r: sum(v) / len(v) >= 0.5 for r, v in share.items()}

    def rank(i: int) -> str:
        return hashlib.sha256(meta[i]["hash"].encode()).hexdigest()

    pools = {"conventional": [], "free": []}
    dropped = 0
    for i, m in enumerate(meta):
        if m["history"] < args.min_history:
            continue
        # Filters added after the inputs were built apply here too.
        if not keep_target(m["target"]):
            dropped += 1
            continue
        if conventional_repo[m["repo"]]:
            if m["type"] is not None:
                pools["conventional"].append(i)
        else:
            pools["free"].append(i)

    chosen = []
    for kind, want in (("conventional", args.conventional), ("free", args.free)):
        per_repo = collections.Counter()
        per_type = collections.Counter()
        type_cap = int(want * args.max_type_share) if kind == "conventional" else want
        taken = 0
        for i in sorted(pools[kind], key=rank):
            if taken == want:
                break
            if per_repo[meta[i]["repo"]] < args.per_repo and per_type[meta[i]["type"]] < type_cap:
                # Checked last: it reads the inputs.
                if args.supported_only and unsupported(
                    meta[i]["target"], evidence(json.loads(input_line(i)))
                ):
                    dropped += 1
                    continue
                per_repo[meta[i]["repo"]] += 1
                per_type[meta[i]["type"]] += 1
                chosen.append((kind, i))
                taken += 1
        print(f"{kind}: {taken} commits from {len(per_repo)} repos", file=sys.stderr)
        print(f"  {dropped} dropped by the target filters so far", file=sys.stderr)
        if kind == "conventional":
            print(f"  types: {dict(per_type.most_common())}", file=sys.stderr)

    args.out.mkdir(parents=True, exist_ok=True)
    with open(args.out / "inputs.jsonl", "w") as fi, open(args.out / "meta.jsonl", "w") as fm:
        for kind, i in chosen:
            fi.write(input_line(i) + "\n")
            fm.write(json.dumps({**meta[i], "set": kind}, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()

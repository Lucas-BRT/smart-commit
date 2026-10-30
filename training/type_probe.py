"""How far a plain classifier gets on the commit type (training H13, C).

A bag of features (the paths, the file statuses, the words of the added and removed lines, the
line counts and the types the history uses) and a logistic regression, trained on a selection
of `sc-training select` and read on the Conventional commits of an evaluation set, like the
models: `type_match` and `type_macro_f1`. It tells whether the type has headroom a separate
classifier could use, or whether the labels are the limit.

    uv run --locked --with scikit-learn python -I type_probe.py [--by-file] TRAIN_DIR EVAL_DIR [N] [PREDICTIONS]

`scikit-learn` is not in uv.lock: this is a probe, not part of the training. With PREDICTIONS,
the classifier's types for the evaluation set go to that file as JSON, and its probabilities to
PREDICTIONS.proba.
"""

import collections
import json
import math
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import type_of  # noqa: E402

WORD = re.compile(r"[A-Za-z_][A-Za-z0-9_]{1,24}")
CHUNK = 4000


def bucket(n: int) -> int:
    return 0 if n <= 0 else int(math.log2(n)) + 1


def by_file_tokens(inputs: dict, files: list[str]) -> list[str]:
    """The types of the past commits that touched the same files (`same:`) or the same
    directories (`near:`); the history's file lists run parallel to its subjects."""
    subjects = inputs.get("subjects") or []
    touched = inputs.get("history_files") or []
    dirs = {f.rsplit("/", 1)[0] if "/" in f else "" for f in files}
    out = []
    latest = True
    for subject, past in zip(subjects, touched):
        kind = type_of(subject)
        if not kind:
            continue
        if set(past) & set(files):
            out.append(f"same:{kind}")
            if latest:
                out.append(f"latest:{kind}")
                latest = False
        elif dirs & {f.rsplit("/", 1)[0] if "/" in f else "" for f in past}:
            out.append(f"near:{kind}")
    return out


def tokens(inputs: dict, by_file: bool = False) -> list[str]:
    """The features of one commit, as strings to hash (`by_file`: and the types of the past
    commits on the same files)."""
    out = []
    staged = {s["path"]: s["status"] for s in inputs.get("staged") or []}
    out.append(f"nfiles:{bucket(len(inputs['files']))}")
    for f in inputs["files"]:
        path = f["path"]
        parts = path.split("/")
        out.append(f"top:{parts[0]}")
        out.append(f"dir:{'/'.join(parts[:-1])}")
        name = parts[-1]
        out.append(f"name:{name}")
        if "." in name:
            out.append(f"ext:{name.rsplit('.', 1)[1].lower()}")
        out.extend(f"p:{w.lower()}" for w in WORD.findall(path))
        out.append(f"status:{staged.get(path, 'modified')}")
        added, removed = (f.get("lines") or [0, 0])[:2]
        out.append(f"add:{bucket(added)}")
        out.append(f"del:{bucket(removed)}")
        for line in (f.get("hunks") or "")[:3000].splitlines():
            if line.startswith("+") and not line.startswith("+++"):
                out.extend(f"+{w.lower()}" for w in WORD.findall(line))
            elif line.startswith("-") and not line.startswith("---"):
                out.extend(f"-{w.lower()}" for w in WORD.findall(line))
    history = collections.Counter(t for t in map(type_of, inputs.get("subjects") or []) if t)
    for kind, n in history.items():
        out.extend([f"hist:{kind}"] * min(n, 5))
    if by_file:
        out.extend(by_file_tokens(inputs, [f["path"] for f in inputs["files"]]))
    return out


def conventional_type(meta: dict) -> str | None:
    """The real commit's type, when the commit is a Conventional one."""
    if meta.get("set") != "conventional":
        return None
    return (meta.get("type") or "").lower() or None


def load(inputs_path: Path, meta_path: Path, limit: int | None = None, by_file: bool = False):
    """The hashed features and the types, a chunk at a time: the inputs do not fit in memory."""
    import scipy.sparse as sp
    from sklearn.feature_extraction import FeatureHasher

    hasher = FeatureHasher(n_features=2**20, input_type="string", alternate_sign=False)
    chunks, types, batch = [], [], []
    with open(inputs_path) as inputs, open(meta_path) as metas:
        for line, meta_line in zip(inputs, metas):
            kind = conventional_type(json.loads(meta_line))
            if not kind:
                continue
            batch.append(tokens(json.loads(line), by_file))
            types.append(kind)
            if len(batch) == CHUNK:
                chunks.append(hasher.transform(batch))
                batch = []
            if limit and len(types) >= limit:
                break
    if batch:
        chunks.append(hasher.transform(batch))
    return sp.vstack(chunks).tocsr(), types


def main() -> None:
    import numpy as np
    from sklearn.feature_extraction.text import TfidfTransformer
    from sklearn.linear_model import SGDClassifier
    from sklearn.metrics import f1_score

    by_file = "--by-file" in sys.argv
    sys.argv = [a for a in sys.argv if a != "--by-file"]
    train, evaluation = Path(sys.argv[1]), Path(sys.argv[2])
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else None
    out = Path(sys.argv[4]) if len(sys.argv) > 4 else None
    x_train, y_train = load(train / "inputs.jsonl", train / "meta.jsonl", limit, by_file)
    x_eval, y_eval = load(evaluation / "inputs.jsonl", evaluation / "meta.jsonl", by_file=by_file)
    print(f"train {len(y_train)} commits, eval {len(y_eval)}", flush=True)
    tfidf = TfidfTransformer(sublinear_tf=True)
    x_train = tfidf.fit_transform(x_train)
    x_eval = tfidf.transform(x_eval)
    for alpha in (3e-5, 1e-5, 3e-6, 1e-6):
        clf = SGDClassifier(loss="log_loss", alpha=alpha, max_iter=40, tol=1e-4, random_state=0)
        clf.fit(x_train, y_train)
        pred = [str(p) for p in clf.predict(x_eval)]
        match = float(np.mean([p == r for p, r in zip(pred, y_eval)]))
        macro = f1_score(y_eval, pred, labels=sorted(set(y_eval)), average="macro", zero_division=0)
        print(f"alpha {alpha:g}: type_match {match:.3f}  macro_f1 {macro:.3f}", flush=True)
        if out and alpha == 1e-5:
            out.write_text(json.dumps(pred))
            Path(f"{out}.proba").write_text(json.dumps(
                {"classes": [str(c) for c in clf.classes_], "p": clf.predict_proba(x_eval).round(4).tolist()}
            ))


if __name__ == "__main__":
    main()

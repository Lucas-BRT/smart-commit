"""What the analysis scripts share: the bootstrap behind every interval they print, and the type of a subject.

The scripts that ship alone to Kaggle (`train.py`, `train_dpo.py`, `mine_pairs.py`) do not import this: each must
run by itself.
"""

import random
import re

TYPE = re.compile(r"^([A-Za-z]+)(\([^)]*\))?!?:")


def type_of(subject: str | None) -> str | None:
    """The lowercase Conventional Commits type a subject starts with, or None."""
    match = TYPE.match(subject or "")
    return match.group(1).lower() if match else None


def bootstrap(items: list, stat, seed: int, bootstraps: int) -> list[float]:
    """The sorted values of `stat` over `bootstraps` resamples, with replacement, of `items` (not empty)."""
    rng = random.Random(seed)
    n = len(items)
    return sorted(stat([items[rng.randrange(n)] for _ in range(n)]) for _ in range(bootstraps))


def bounds(values: list[float]) -> tuple[float, float]:
    """The 95% interval of sorted bootstrap values: the 2.5th and the 97.5th percentile."""
    return values[int(0.025 * len(values))], values[int(0.975 * len(values)) - 1]

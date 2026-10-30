"""Checks `common`, the bootstrap and the subject type the analysis scripts share.

    python -I test_common.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import bootstrap, bounds, type_of  # noqa: E402


def test_type_of_reads_the_conventional_type() -> None:
    assert type_of("feat: add x") == "feat"
    assert type_of("Fix(parser)!: y") == "fix"
    assert type_of("tests: z") == "tests"
    assert type_of("add x") is None
    assert type_of("") is None and type_of(None) is None


def test_bootstrap_is_sorted_and_the_same_for_the_same_seed() -> None:
    items = [0.0, 1.0, 2.0, 3.0, 4.0]
    values = bootstrap(items, lambda sample: sum(sample) / len(sample), 7, 200)
    assert len(values) == 200 and values == sorted(values)
    assert values == bootstrap(items, lambda sample: sum(sample) / len(sample), 7, 200)
    assert values != bootstrap(items, lambda sample: sum(sample) / len(sample), 8, 200)
    assert 0.0 <= values[0] and values[-1] <= 4.0


def test_a_constant_sample_has_a_point_interval() -> None:
    values = bootstrap([2.0] * 10, lambda sample: sum(sample) / len(sample), 7, 50)
    assert bounds(values) == (2.0, 2.0)


def test_bounds_are_the_2_5th_and_97_5th_percentile() -> None:
    values = [float(n) for n in range(2000)]
    assert bounds(values) == (50.0, 1949.0)


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_"):
            test()
            print(f"ok {name}")

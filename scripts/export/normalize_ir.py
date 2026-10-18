"""Makes OpenVINO IR files independent of the process that wrote them.

OpenVINO names the nodes it creates itself `<Type>_<n>`, with `n` from a
process-wide counter, so the same export gives different names (and bytes)
depending on how many nodes the process created before: the graph and the
weights are identical, only these names differ. This renames them in order
of appearance, which is the same everywhere. Nothing refers to nodes by
these names (edges use ids; inputs and outputs use tensor names).

    python normalize_ir.py FILE.xml...
"""

import re
import sys

AUTO_NAME = re.compile(r'name="([A-Za-z][A-Za-z0-9]*)_(\d+)"')


def normalize(text: str) -> str:
    renamed: dict[str, str] = {}

    def rename(match: re.Match) -> str:
        original = match.group(0)
        if original not in renamed:
            renamed[original] = f'name="{match.group(1)}_{len(renamed)}"'
        return renamed[original]

    return AUTO_NAME.sub(rename, text)


def main() -> None:
    for path in sys.argv[1:]:
        with open(path, encoding="utf-8") as f:
            text = f.read()
        with open(path, "w", encoding="utf-8", newline="") as f:
            f.write(normalize(text))


if __name__ == "__main__":
    main()

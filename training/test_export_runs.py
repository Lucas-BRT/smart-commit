"""Checks `export_runs` with a stand-in for export_gguf.py.

    python -I test_export_runs.py
"""

import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent


def setup(d: Path, names: list[str]) -> Path:
    runs = d / "runs"
    for name in names:
        (runs / name / "adapter").mkdir(parents=True)
    (runs / "notes").mkdir()  # a directory without an adapter is not a run
    (runs / "b.log").write_text("log")
    return runs


def exporter(d: Path, fail_on: str = "") -> Path:
    """Writes the GGUF it is asked for, with the adapters it got; fails for `fail_on`."""
    fake = d / "export.py"
    fake.write_text(
        "import sys\n"
        "out = sys.argv[2]\n"
        f"if {fail_on!r} and {fail_on!r} in out:\n"
        "    sys.exit('error: no memory')\n"
        "adapters = [sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == '--adapter']\n"
        "open(out, 'w').write(' '.join(adapters))\n"
        f"open({str(d / 'calls')!r}, 'a').write('x')\n"
    )
    return fake


def run(d: Path, runs: Path, fake: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-I", str(HERE / "export_runs.py"), "base", "sft-adapter", str(runs),
         "--llama-src", "src", "--llama-bin", "bin", "--exporter", str(fake)],
        capture_output=True, text=True, cwd=HERE)


def test_each_run_is_exported_once_with_the_sft_adapter_first() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        runs = setup(d, ["b03", "b01"])
        fake = exporter(d)
        first = run(d, runs, fake)
        assert first.returncode == 0, first.stderr
        # NAME=GGUF, in the order of the names, ready for sc-training compare.
        assert first.stdout.splitlines() == [f"b01={runs.resolve() / 'b01/model-q4_k_m.gguf'}",
                                             f"b03={runs.resolve() / 'b03/model-q4_k_m.gguf'}"]
        assert (runs / "b01/model-q4_k_m.gguf").read_text() == f"sft-adapter {runs / 'b01/adapter'}"
        assert (d / "calls").read_text() == "xx"
        # Again: nothing is made twice, the list is the same.
        second = run(d, runs, fake)
        assert second.stdout == first.stdout and (d / "calls").read_text() == "xx"
        assert "already exported" in second.stderr


def test_a_failed_export_stops_with_the_reason_and_leaves_no_half_made_file() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        runs = setup(d, ["a", "b"])
        done = run(d, runs, exporter(d, fail_on="/b/"))
        assert done.returncode != 0 and "no memory" in done.stderr
        assert (runs / "a/model-q4_k_m.gguf").exists() and not (runs / "b/model-q4_k_m.gguf").exists()
        empty = d / "empty"
        empty.mkdir()
        assert "no run with an adapter" in run(d, empty, exporter(d)).stderr


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

"""Checks `mine_pairs` against a stand-in smart-commit and llama-server.

    python -I test_mine_pairs.py
"""

import json
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from mine_pairs import mine, repos_of, select  # noqa: E402


def write_set(d: Path, repos: list[str]) -> None:
    d.mkdir(parents=True, exist_ok=True)
    (d / "inputs.jsonl").write_text("".join(json.dumps({"n": n}) + "\n" for n in range(len(repos))))
    (d / "meta.jsonl").write_text(
        "".join(json.dumps({"repo": r, "target": f"feat: t{n}", "set": "conventional"}) + "\n"
                for n, r in enumerate(repos))
    )


def fake_smart_commit(d: Path) -> str:
    """Answers `prompt --inputs F ...` with a prompt per line of F, whose
    user message is `u` and the line's number from 0."""
    fake = d / "smart-commit"
    fake.write_text(
        '#!/bin/sh\nn=0\nwhile read -r line; do echo "{\\"system\\": \\"S\\", \\"user\\": \\"u$n\\"}"; '
        'n=$((n+1)); done < "$3"\n'
    )
    fake.chmod(0o755)
    return str(fake)


class Llama(BaseHTTPRequestHandler):
    seeds: list[int] = []

    def do_POST(self) -> None:  # noqa: N802
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        Llama.seeds.append(body["seed"])
        reply = {"choices": [{"message": {"content": f"fix: seed {body['seed']}"}}]}
        data = json.dumps(reply).encode()
        self.send_response(200)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args) -> None:
        pass


def serve() -> tuple[HTTPServer, str]:
    server = HTTPServer(("127.0.0.1", 0), Llama)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_port}"


def test_the_repositories_of_the_evaluation_sets_are_left_out() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        write_set(d / "src", ["a/x", "eval/y", "b/z", "eval-short/w"])
        write_set(d / "eval", ["eval/y"])
        write_set(d / "short", ["eval-short/w"])
        exclude = repos_of([d / "eval", d / "short"])
        assert exclude == {"eval/y", "eval-short/w"}
        assert select(d / "src", d / "out", exclude, 0) == 2
        kept = [json.loads(line)["repo"] for line in (d / "out/meta.jsonl").read_text().splitlines()]
        assert kept == ["a/x", "b/z"]
        # The inputs stay in step with the meta.
        assert [json.loads(line)["n"] for line in (d / "out/inputs.jsonl").read_text().splitlines()] == [0, 2]
        assert select(d / "src", d / "limited", set(), 3) == 3


def test_replies_are_sampled_with_fixed_seeds_in_the_order_of_the_inputs() -> None:
    server, url = serve()
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        write_set(d / "src", ["a/x", "b/y", "c/z"])
        select(d / "src", d / "out", set(), 0)
        binary = fake_smart_commit(d)
        Llama.seeds = []
        assert mine(d / "out", binary, url, "full", "plain", 2, 0.7, 0.95, 7, workers=2) == 3
        rows = [json.loads(line) for line in (d / "out/candidates.jsonl").read_text().splitlines()]
        assert [r["system"] for r in rows] == ["S", "S", "S"]
        assert [r["user"] for r in rows] == ["u0", "u1", "u2"]
        assert all(len(r["replies"]) == 2 for r in rows)
        # Seeds depend on the input and the sample, not on the thread.
        assert rows[1]["replies"] == [f"fix: seed {7 + 1000}", f"fix: seed {7 + 1001}"]
        assert sorted(Llama.seeds) == sorted(7 + 1000 * n + k for n in range(3) for k in range(2))
    server.shutdown()


def test_a_stopped_run_resumes_after_the_inputs_it_finished() -> None:
    server, url = serve()
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        write_set(d / "src", ["a/x", "b/y", "c/z"])
        select(d / "src", d / "out", set(), 0)
        binary = fake_smart_commit(d)
        mine(d / "out", binary, url, "full", "plain", 1, 0.7, 0.95, 0, workers=1)
        complete = (d / "out/candidates.jsonl").read_text()
        # Cut after the first input: only the other two are sampled again.
        first = complete.splitlines()[0] + "\n"
        (d / "out/candidates.jsonl").write_text(first)
        Llama.seeds = []
        assert mine(d / "out", binary, url, "full", "plain", 1, 0.7, 0.95, 0, workers=1) == 2
        assert (d / "out/candidates.jsonl").read_text() == complete
        assert sorted(Llama.seeds) == [1000, 2000]
        # Nothing left to do.
        assert mine(d / "out", binary, url, "full", "plain", 1, 0.7, 0.95, 0, workers=1) == 0
    server.shutdown()


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)

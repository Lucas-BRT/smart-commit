"""Times llama-server on prompts printed by `smart-commit prompt`.

usage: python3 -I time-llama-server.py MODEL.gguf PROMPTS_DIR LABEL [server flags...]

PROMPTS_DIR holds one `smart-commit prompt` output per file (*.txt). The
server is the one `smart-commit setup` installs, or $LLAMA_SERVER. Requests
are the ones smart-commit sends (greedy, seeded, no prompt cache);
$EXTRA_BODY (JSON) is merged into them. Prints the load time and the mean
prefill and decode times, then one line per prompt with its first reply line.
See docs/training.md.
"""
import glob
import http.client
import json
import os
import socket
import subprocess
import sys
import tempfile
import time

SERVER = os.environ.get(
    "LLAMA_SERVER",
    os.path.expanduser("~/.local/share/smart-commit/llama.cpp/llama-b11381/llama-server"),
)


class UnixConnection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost")
        self.socket_path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.connect(self.socket_path)


def parse(path):
    text = open(path, encoding="utf-8").read()
    system = text.split("===== system =====\n", 1)[1].split("===== user =====\n")[0]
    user = text.split("===== user =====\n", 1)[1].split("\n===== ~")[0]
    return system.rstrip("\n") + "\n", user


def main():
    model, prompts_dir, label, extra = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
    workdir = tempfile.mkdtemp()
    sock = os.path.join(workdir, "server.sock")
    log = os.path.join(workdir, "server.log")
    args = [SERVER, "--model", model, "--host", sock, "--ctx-size", "1152",
            "--parallel", "1", "--no-webui", "--offline", *extra]
    started = time.time()
    server = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=open(log, "w"))
    while True:
        try:
            conn = UnixConnection(sock)
            conn.request("GET", "/health")
            reply = conn.getresponse()
            reply.read()
            if reply.status == 200:
                break
        except OSError:
            pass
        if server.poll() is not None:
            print(open(log).read()[-2000:])
            sys.exit(1)
        time.sleep(0.01)
    load_ms = (time.time() - started) * 1000

    rows = []
    for path in sorted(glob.glob(os.path.join(prompts_dir, "*.txt"))):
        system, user = parse(path)
        body = {
            "messages": [{"role": "system", "content": system},
                         {"role": "user", "content": user}],
            "max_tokens": 128, "temperature": 0, "top_k": 1, "seed": 0,
            "cache_prompt": False,
        }
        body.update(json.loads(os.environ.get("EXTRA_BODY", "{}")))
        conn = UnixConnection(sock)
        conn.request("POST", "/v1/chat/completions", json.dumps(body),
                     {"Content-Type": "application/json"})
        value = json.loads(conn.getresponse().read())
        timings = value["timings"]
        content = value["choices"][0]["message"]["content"].strip()
        first_line = (content.splitlines() or [""])[0][:70]
        rows.append((os.path.basename(path), timings["prompt_n"], timings["prompt_ms"],
                     timings["predicted_n"], timings["predicted_ms"], first_line))
    server.terminate()
    server.wait()

    prefill = sum(r[2] for r in rows) / len(rows)
    decode = sum(r[4] for r in rows) / len(rows)
    print(f"== {label}: load {load_ms:.0f} ms, prefill {prefill:.0f} ms, "
          f"decode {decode:.0f} ms, total/msg {load_ms + prefill + decode:.0f} ms")
    for name, n_in, ms_in, n_out, ms_out, first in rows:
        print(f"   {name:14} in={n_in:4} {ms_in:6.0f}ms out={n_out:3} {ms_out:6.0f}ms  {first}")


if __name__ == "__main__":
    main()

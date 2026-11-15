#!/usr/bin/env python3
"""Snake high-score backend.

Standard library only. Listens on 127.0.0.1 on $PORT (default 8765).
Scores are stored in scores.txt next to this file, one "<name> <score>" per line.
"""

import os
import re
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

HOST = "127.0.0.1"
DEFAULT_PORT = 8765
TOP_N = 10
MAX_BODY = 1024  # bytes; a valid score line is far shorter than this

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
SCORES_FILE = os.path.join(BASE_DIR, "scores.txt")

# "<name> <score>": name is one or more non-whitespace chars, score is digits only.
SCORE_LINE_RE = re.compile(r"(\S+) ([0-9]+)")

_lock = threading.Lock()


def read_scores():
    """Return list of (name, score) tuples from scores.txt. Caller holds _lock."""
    if not os.path.exists(SCORES_FILE):
        return []
    entries = []
    with open(SCORES_FILE, "r", encoding="utf-8") as f:
        for line in f:
            m = SCORE_LINE_RE.fullmatch(line.strip())
            if m:
                entries.append((m.group(1), int(m.group(2))))
    return entries


def append_score(name, score):
    """Append one score line to scores.txt. Caller holds _lock."""
    prefix = ""
    if os.path.exists(SCORES_FILE) and os.path.getsize(SCORES_FILE) > 0:
        with open(SCORES_FILE, "rb") as f:
            f.seek(-1, os.SEEK_END)
            if f.read(1) != b"\n":
                prefix = "\n"
    with open(SCORES_FILE, "a", encoding="utf-8") as f:
        f.write(f"{prefix}{name} {score}\n")


class Handler(BaseHTTPRequestHandler):
    server_version = "SnakeScores/1.0"

    # ---- helpers -------------------------------------------------------

    def _send(self, status, body=b"", content_type="text/plain; charset=utf-8"):
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        if status == 204:
            self.end_headers()
            return
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body and self.command != "HEAD":
            self.wfile.write(body)

    def send_error(self, code, message=None, explain=None):
        # Route all error responses (including the stdlib's own, e.g. 501)
        # through _send so they carry the CORS header.
        self._send(code, (message or self.responses.get(code, ("",))[0]).encode() + b"\n")

    def _path(self):
        return urlsplit(self.path).path

    # ---- verbs ---------------------------------------------------------

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self._path() != "/scores":
            return self._send(404, b"Not found\n")
        with _lock:
            entries = read_scores()
        # Stable sort: highest score first, ties keep file order.
        entries.sort(key=lambda e: e[1], reverse=True)
        top = entries[:TOP_N]
        body = "".join(f"{name} {score}\n" for name, score in top).encode("utf-8")
        self._send(200, body)

    def do_POST(self):
        if self._path() != "/scores":
            return self._send(404, b"Not found\n")
        length = self.headers.get("Content-Length")
        try:
            length = int(length) if length is not None else 0
        except ValueError:
            return self._send(400, b"Bad request\n")
        if length < 0 or length > MAX_BODY:
            return self._send(400, b"Bad request\n")
        raw = self.rfile.read(length)
        try:
            text = raw.decode("utf-8").strip()
        except UnicodeDecodeError:
            return self._send(400, b"Bad request\n")
        m = SCORE_LINE_RE.fullmatch(text)
        if not m:
            return self._send(400, b"Bad request\n")
        name, score = m.group(1), int(m.group(2))
        with _lock:
            append_score(name, score)
        self._send(204)

    def do_PUT(self):
        self._method_not_allowed()

    do_DELETE = do_PUT
    do_PATCH = do_PUT

    def _method_not_allowed(self):
        if self._path() == "/scores":
            return self._send(405, b"Method not allowed\n")
        return self._send(404, b"Not found\n")

    def log_message(self, fmt, *args):
        sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main():
    port = int(os.environ.get("PORT", DEFAULT_PORT))
    httpd = Server((HOST, port), Handler)
    print(f"Listening on http://{HOST}:{port}", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()

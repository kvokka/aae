#!/usr/bin/env python3
"""Score backend for the browser snake game.

Standard library only. Listens on 127.0.0.1:$PORT (default 8765).
Scores are stored in scores.txt next to this file, one "<name> <score>" per line.
"""

import os
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
SCORES_PATH = os.path.join(HERE, "scores.txt")
HOST = "127.0.0.1"
DEFAULT_PORT = 8765
TOP_N = 10

_lock = threading.Lock()
_SCORE_RE = re.compile(r"[0-9]+\Z")


def read_scores():
    """Return list of (name, score_int, line) tuples in file order. Caller holds _lock."""
    try:
        with open(SCORES_PATH, "r", encoding="utf-8") as f:
            raw_lines = f.read().split("\n")
    except FileNotFoundError:
        return []
    entries = []
    for raw in raw_lines:
        line = raw.rstrip("\r")
        parts = line.split(" ")
        if len(parts) != 2 or not parts[0] or not _SCORE_RE.match(parts[1]):
            continue
        entries.append((parts[0], int(parts[1]), f"{parts[0]} {parts[1]}"))
    return entries


def append_score(name, score):
    """Append one line to scores.txt. Caller holds _lock."""
    needs_newline = False
    try:
        with open(SCORES_PATH, "rb") as f:
            f.seek(0, os.SEEK_END)
            if f.tell() > 0:
                f.seek(-1, os.SEEK_END)
                needs_newline = f.read(1) != b"\n"
    except FileNotFoundError:
        pass
    with open(SCORES_PATH, "a", encoding="utf-8") as f:
        if needs_newline:
            f.write("\n")
        f.write(f"{name} {score}\n")


def parse_score_body(body_bytes):
    """Parse '<name> <score>'. Returns (name, score_str) or None if invalid."""
    try:
        text = body_bytes.decode("utf-8")
    except UnicodeDecodeError:
        return None
    text = text.rstrip("\r\n")
    parts = text.split(" ")
    if len(parts) != 2:
        return None
    name, score = parts
    if not name or any(ch.isspace() for ch in name):
        return None
    if not _SCORE_RE.match(score):
        return None
    return name, str(int(score))


class ScoreHandler(BaseHTTPRequestHandler):
    server_version = "SnakeScores/1.0"

    # ---- helpers -------------------------------------------------------
    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")

    def _respond(self, status, body=b"", content_type="text/plain; charset=utf-8"):
        self.send_response(status)
        self._cors()
        if status != 204:
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body and self.command != "HEAD":
            self.wfile.write(body)

    def send_error(self, code, message=None, explain=None):
        # Route every error (including the stdlib's 501 for unknown methods)
        # through the same path so it carries the CORS header.
        self.close_connection = True
        self._respond(code, (message or self.responses.get(code, ("",))[0]).encode("utf-8") + b"\n")

    # ---- routing -------------------------------------------------------
    def _path(self):
        return urlsplit(self.path).path

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self._path() != "/scores":
            self._respond(404, b"Not Found\n")
            return
        with _lock:
            entries = read_scores()
        # Stable sort: equal scores keep file (earlier-written first) order.
        entries.sort(key=lambda e: -e[1])
        body = "".join(e[2] + "\n" for e in entries[:TOP_N]).encode("utf-8")
        self._respond(200, body)

    def do_POST(self):
        if self._path() != "/scores":
            self._respond(404, b"Not Found\n")
            return
        try:
            length = int(self.headers.get("Content-Length", "0") or "0")
        except ValueError:
            length = -1
        if length < 0:
            self._respond(400, b"Bad Request\n")
            return
        body = self.rfile.read(length) if length else b""
        parsed = parse_score_body(body)
        if parsed is None:
            self._respond(400, b"Bad Request: expected '<name> <score>'\n")
            return
        name, score = parsed
        with _lock:
            append_score(name, score)
        self._respond(204)

    def do_PUT(self):
        self._respond(405, b"Method Not Allowed\n")

    do_DELETE = do_PATCH = do_PUT

    def log_message(self, fmt, *args):
        pass


class Server(ThreadingHTTPServer):
    daemon_threads = True


def main():
    port = int(os.environ.get("PORT", str(DEFAULT_PORT)))
    httpd = Server((HOST, port), ScoreHandler)
    print(f"Serving scores on http://{HOST}:{port}/scores (file: {SCORES_PATH})", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()

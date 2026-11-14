#!/usr/bin/env python3
"""Minimal score server for the snake game (standard library only).

Endpoints:
  POST /scores  body "<name> <score>" (plain text) -> 204, appends to scores.txt
  GET  /scores  -> 200, plain text, ten best "<name> <score>" lines, highest first
  OPTIONS *     -> 204 with CORS headers

Listens on 127.0.0.1 only, on $PORT (default 8765).
"""

import os
import re
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
SCORES_FILE = os.path.join(HERE, "scores.txt")
HOST = "127.0.0.1"
DEFAULT_PORT = 8765
TOP_N = 10
MAX_BODY = 4096

SCORE_RE = re.compile(r"[0-9]+")
_lock = threading.Lock()


def parse_entry(text):
    """Return (name, score_str) for a valid "<name> <score>" line, else None."""
    parts = text.split(" ")
    if len(parts) != 2:
        return None
    name, score = parts
    if not name or any(c.isspace() for c in name):
        return None
    if not SCORE_RE.fullmatch(score):
        return None
    return name, score


def append_score(name, score):
    with _lock:
        needs_newline = False
        if os.path.exists(SCORES_FILE) and os.path.getsize(SCORES_FILE) > 0:
            with open(SCORES_FILE, "rb") as f:
                f.seek(-1, os.SEEK_END)
                needs_newline = f.read(1) != b"\n"
        with open(SCORES_FILE, "a", encoding="utf-8") as f:
            if needs_newline:
                f.write("\n")
            f.write(f"{name} {score}\n")


def top_scores():
    with _lock:
        if not os.path.exists(SCORES_FILE):
            return []
        with open(SCORES_FILE, "r", encoding="utf-8") as f:
            lines = f.read().splitlines()
    entries = []
    for line in lines:
        parsed = parse_entry(line.strip())
        if parsed:
            entries.append((int(parsed[1]), parsed[0]))
    entries.sort(key=lambda e: e[0], reverse=True)
    return [f"{name} {score}" for score, name in entries[:TOP_N]]


class Handler(BaseHTTPRequestHandler):
    server_version = "SnakeScores/1.0"

    def end_headers(self):
        # Every response, including errors, carries the CORS header.
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def _reply(self, status, body=None, content_type="text/plain; charset=utf-8"):
        data = body.encode("utf-8") if body else b""
        self.send_response(status)
        if status != 204:
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if data:
            self.wfile.write(data)

    def _path(self):
        return self.path.split("?", 1)[0]

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self._path() != "/scores":
            self._reply(404, "Not found\n")
            return
        lines = top_scores()
        body = "".join(line + "\n" for line in lines)
        self._reply(200, body)

    def do_POST(self):
        if self._path() != "/scores":
            self._reply(404, "Not found\n")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self._reply(400, "Bad request\n")
            return
        if length < 0 or length > MAX_BODY:
            self._reply(400, "Bad request\n")
            return
        raw = self.rfile.read(length) if length else b""
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            self._reply(400, "Bad request\n")
            return
        entry = parse_entry(text.rstrip("\r\n"))
        if entry is None:
            self._reply(400, "Bad request: expected '<name> <score>'\n")
            return
        append_score(*entry)
        self._reply(204)

    def do_PUT(self):
        self._reply(405, "Method not allowed\n")

    do_DELETE = do_PATCH = do_PUT


def main():
    raw_port = os.environ.get("PORT", "").strip()
    try:
        port = int(raw_port) if raw_port else DEFAULT_PORT
    except ValueError:
        print(f"Invalid PORT: {raw_port!r}", file=sys.stderr)
        sys.exit(1)
    ThreadingHTTPServer.daemon_threads = True
    httpd = ThreadingHTTPServer((HOST, port), Handler)
    print(f"Listening on http://{HOST}:{port}", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()

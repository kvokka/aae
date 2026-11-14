#!/usr/bin/env python3
"""Score server for the browser snake game.

Standard library only. Listens on 127.0.0.1 on $PORT (default 8765).
Scores are kept in scores.txt next to this file, one "<name> <score>" per line.

  POST /scores  body "<name> <score>"  -> 204, or 400 if malformed
  GET  /scores                         -> 200 text/plain, top 10, highest first
  OPTIONS *                            -> 204 (CORS preflight)
"""

import os
import re
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
SCORES_PATH = os.path.join(HERE, "scores.txt")
TOP_N = 10
HOST = "127.0.0.1"
DEFAULT_PORT = 8765

SCORE_RE = re.compile(r"[0-9]+")
MAX_BODY = 4096

_file_lock = threading.Lock()


def parse_entry(text):
    """Return (name, score_int) for a '<name> <score>' string, or None."""
    fields = text.split()
    if len(fields) != 2:
        return None
    name, score = fields
    if not SCORE_RE.fullmatch(score):
        return None
    return name, int(score)


def append_score(name, score):
    with _file_lock:
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


def top_scores():
    with _file_lock:
        try:
            with open(SCORES_PATH, "r", encoding="utf-8") as f:
                lines = f.read().splitlines()
        except FileNotFoundError:
            return []
    entries = []
    for line in lines:
        entry = parse_entry(line)
        if entry is not None:
            entries.append(entry)
    # Stable sort: ties keep file order.
    entries.sort(key=lambda e: e[1], reverse=True)
    return entries[:TOP_N]


class Handler(BaseHTTPRequestHandler):
    server_version = "SnakeScores/1.0"

    def end_headers(self):
        # Every response, including errors, carries the CORS header.
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def _respond(self, code, body=None, content_type="text/plain; charset=utf-8"):
        payload = b"" if body is None else body.encode("utf-8")
        self.send_response(code)
        if code != 204:
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        if payload:
            self.wfile.write(payload)

    def _path(self):
        return urlsplit(self.path).path

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self._path() != "/scores":
            self._respond(404, "Not found\n")
            return
        lines = "".join(f"{name} {score}\n" for name, score in top_scores())
        self._respond(200, lines)

    def do_POST(self):
        if self._path() != "/scores":
            self._respond(404, "Not found\n")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self._respond(400, "Bad request\n")
            return
        if length < 0 or length > MAX_BODY:
            self._respond(400, "Bad request\n")
            return
        raw = self.rfile.read(length) if length else b""
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            self._respond(400, "Bad request\n")
            return
        entry = parse_entry(text)
        if entry is None:
            self._respond(400, "Bad request\n")
            return
        name, score = entry
        append_score(name, score)
        self._respond(204)

    def _not_allowed(self):
        self._respond(405, "Method not allowed\n")

    do_PUT = do_DELETE = do_PATCH = _not_allowed


def main():
    raw_port = os.environ.get("PORT", str(DEFAULT_PORT))
    try:
        port = int(raw_port)
    except ValueError:
        sys.exit(f"Invalid PORT: {raw_port!r}")
    server = ThreadingHTTPServer((HOST, port), Handler)
    server.daemon_threads = True
    print(f"Serving scores on http://{HOST}:{port}/scores (file: {SCORES_PATH})", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

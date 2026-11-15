#!/usr/bin/env python3
"""Score backend for the browser snake game.

Standard library only. Listens on 127.0.0.1:$PORT (default 8765).

  POST /scores  body "<name> <score>" (name has no spaces, score is a
                non-negative integer) -> appends a line to scores.txt, 204.
                Anything else -> 400.
  GET  /scores  -> 200, text/plain, the ten best lines (highest score first).
  OPTIONS *     -> 204 with CORS headers.
  anything else -> 404.

Every response carries Access-Control-Allow-Origin: *.
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
MAX_BODY = 1024  # bytes; a valid score line is far smaller than this

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
SCORES_PATH = os.path.join(BASE_DIR, "scores.txt")

_file_lock = threading.Lock()
_SCORE_RE = re.compile(r"[0-9]+", re.ASCII)


def parse_score_line(text):
    """Parse 'name score'. Return (name, score_int) or None if invalid."""
    parts = text.split()
    if len(parts) != 2:
        return None
    name, score = parts
    if not name.isprintable() or not _SCORE_RE.fullmatch(score):
        return None
    return name, int(score)


def append_score(name, score):
    with _file_lock:
        path = SCORES_PATH
        needs_newline = False
        if os.path.exists(path) and os.path.getsize(path) > 0:
            with open(path, "rb") as f:
                f.seek(-1, os.SEEK_END)
                needs_newline = f.read(1) != b"\n"
        with open(path, "a", encoding="utf-8") as f:
            if needs_newline:
                f.write("\n")
            f.write(f"{name} {score}\n")


def top_scores(limit=TOP_N):
    with _file_lock:
        try:
            with open(SCORES_PATH, "r", encoding="utf-8") as f:
                lines = f.read().splitlines()
        except FileNotFoundError:
            return []
    entries = []
    for index, line in enumerate(lines):
        parsed = parse_score_line(line)
        if parsed is not None:
            name, score = parsed
            entries.append((-score, index, f"{name} {score}"))
    entries.sort()  # highest score first; file order breaks ties
    return [text for _, _, text in entries[:limit]]


class ScoreHandler(BaseHTTPRequestHandler):
    server_version = "SnakeScores/1.0"

    def end_headers(self):
        # Every response (including send_error responses) gets CORS.
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def _path(self):
        return urlsplit(self.path).path

    def _send_text(self, status, body):
        data = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if data:
            self.wfile.write(data)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self._path() != "/scores":
            self._send_text(404, "Not Found\n")
            return
        lines = top_scores()
        body = "".join(line + "\n" for line in lines)
        self._send_text(200, body)

    def do_POST(self):
        if self._path() != "/scores":
            self._send_text(404, "Not Found\n")
            return
        try:
            length = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            length = -1
        if length < 0 or length > MAX_BODY:
            self._send_text(400, "Bad Request\n")
            return
        raw = self.rfile.read(length) if length else b""
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            self._send_text(400, "Bad Request\n")
            return
        parsed = parse_score_line(text)
        if parsed is None:
            self._send_text(400, "Bad Request\n")
            return
        name, score = parsed
        append_score(name, score)
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def _method_not_allowed(self):
        if self._path() == "/scores":
            self._send_text(405, "Method Not Allowed\n")
        else:
            self._send_text(404, "Not Found\n")

    do_PUT = do_DELETE = do_PATCH = _method_not_allowed

    def log_message(self, format, *args):
        sys.stderr.write("%s - %s\n" % (self.address_string(), format % args))


class ScoreServer(ThreadingHTTPServer):
    daemon_threads = True


def main():
    raw_port = os.environ.get("PORT", "").strip()
    if raw_port:
        try:
            port = int(raw_port)
        except ValueError:
            sys.exit(f"PORT must be an integer, got {raw_port!r}")
        if not 0 < port < 65536:
            sys.exit(f"PORT out of range: {port}")
    else:
        port = DEFAULT_PORT
    httpd = ScoreServer((HOST, port), ScoreHandler)
    print(f"Serving scores on http://{HOST}:{port}/scores "
          f"(file: {SCORES_PATH})", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()

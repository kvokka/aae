"""Snake high-score server (standard library only).

POST /scores  body: "<name> <score>"  -> appends a line to scores.txt, 204
GET  /scores                          -> 10 best lines, highest first, text/plain
OPTIONS *                             -> 204 (CORS preflight)
"""

import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = "127.0.0.1"
DEFAULT_PORT = 8765
TOP_N = 10
MAX_BODY = 1024

SCORES_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "scores.txt")
_lock = threading.Lock()


def parse_entry(text):
    """Return (name, score) for a valid '<name> <score>' string, else None."""
    parts = text.split()
    if len(parts) != 2:
        return None
    name, score = parts
    if not score.isascii() or not score.isdigit():
        return None
    return name, int(score)


def read_entries():
    """Return all valid (name, score) entries in file order."""
    try:
        with open(SCORES_FILE, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except FileNotFoundError:
        return []
    entries = []
    for line in lines:
        entry = parse_entry(line)
        if entry is not None:
            entries.append(entry)
    return entries


class Handler(BaseHTTPRequestHandler):
    server_version = "SnakeScores/1.0"

    def _send(self, status, body=b"", content_type="text/plain; charset=utf-8"):
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        if status != 204:
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body and status != 204:
            self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path.split("?", 1)[0] != "/scores":
            self._send(404, b"not found\n")
            return
        with _lock:
            entries = read_entries()
        # sorted() is stable, so ties keep submission order.
        entries = sorted(entries, key=lambda e: -e[1])[:TOP_N]
        body = "".join(f"{name} {score}\n" for name, score in entries)
        self._send(200, body.encode("utf-8"))

    def do_POST(self):
        if self.path.split("?", 1)[0] != "/scores":
            self._send(404, b"not found\n")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            length = -1
        if length < 0 or length > MAX_BODY:
            self._send(400, b"malformed body\n")
            return
        raw = self.rfile.read(length)
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            self._send(400, b"malformed body\n")
            return
        entry = parse_entry(text)
        if entry is None:
            self._send(400, b"expected: <name> <score>\n")
            return
        name, score = entry
        with _lock:
            with open(SCORES_FILE, "a", encoding="utf-8") as f:
                f.write(f"{name} {score}\n")
        self._send(204)

    def do_PUT(self):
        self._send(404, b"not found\n")

    do_DELETE = do_PATCH = do_PUT

    def log_message(self, format, *args):
        pass


def main():
    port = int(os.environ.get("PORT", DEFAULT_PORT))
    with ThreadingHTTPServer((HOST, port), Handler) as httpd:
        print(f"serving on http://{HOST}:{port}/scores", flush=True)
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()

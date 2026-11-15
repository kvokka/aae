#!/usr/bin/env python3
"""Small standard-library HTTP backend for the browser snake game."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path


SCORES_FILE = Path(__file__).resolve().parent / "scores.txt"


class SnakeRequestHandler(BaseHTTPRequestHandler):
    def _send_headers(self, status, content_type=None, extra_headers=None):
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        if content_type:
            self.send_header("Content-Type", content_type)
        if extra_headers:
            for name, value in extra_headers.items():
                self.send_header(name, value)
        self.end_headers()

    def do_OPTIONS(self):
        self._send_headers(
            204,
            extra_headers={
                "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
                "Access-Control-Allow-Headers": "Content-Type",
            },
        )

    def do_POST(self):
        if self.path != "/scores":
            self._send_headers(404)
            return

        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError("negative content length")
            body = self.rfile.read(length).decode("utf-8")
            parts = body.split()
            if len(parts) != 2 or not parts[0] or not parts[1].isdigit():
                raise ValueError("expected a name and a whole-number score")
            name, score = parts
            # Names cannot contain whitespace, and scores are non-negative integers.
            with SCORES_FILE.open("a", encoding="utf-8") as scores:
                scores.write(f"{name} {int(score)}\n")
        except (ValueError, UnicodeDecodeError, OSError):
            self._send_headers(400)
            return

        self._send_headers(204)

    def do_GET(self):
        if self.path != "/scores":
            self._send_headers(404)
            return

        entries = []
        try:
            with SCORES_FILE.open("r", encoding="utf-8") as scores:
                for line in scores:
                    parts = line.split()
                    if len(parts) == 2 and parts[1].isdigit():
                        entries.append((int(parts[1]), line.strip()))
        except FileNotFoundError:
            pass

        entries.sort(key=lambda entry: entry[0], reverse=True)
        body = "".join(f"{line}\n" for _, line in entries[:10]).encode("utf-8")
        self._send_headers(200, "text/plain; charset=utf-8")
        self.wfile.write(body)


def main():
    port = int(os.environ.get("PORT", "8765"))
    server = ThreadingHTTPServer(("127.0.0.1", port), SnakeRequestHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

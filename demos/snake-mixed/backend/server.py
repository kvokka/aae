#!/usr/bin/env python3
"""Small plain-text score service for the browser snake game."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path

SCORES_FILE = Path(__file__).resolve().parent / "scores.txt"


class ScoreHandler(BaseHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_POST(self):
        if self.path != "/scores":
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            body = self.rfile.read(length).decode("utf-8")
            parts = body.split()
            if (len(parts) != 2 or not parts[0] or " " in parts[0]
                    or not parts[1].isascii() or not parts[1].isdigit()):
                raise ValueError
            line = f"{parts[0]} {int(parts[1])}"
        except (ValueError, UnicodeDecodeError):
            self.send_error(400)
            return
        try:
            with SCORES_FILE.open("a", encoding="utf-8") as scores:
                scores.write(line + "\n")
        except OSError:
            self.send_error(500)
            return
        self.send_response(204)
        self.end_headers()

    def do_GET(self):
        if self.path != "/scores":
            self.send_error(404)
            return
        try:
            with SCORES_FILE.open("r", encoding="utf-8") as scores:
                entries = []
                for order, raw_line in enumerate(scores):
                    parts = raw_line.strip().split()
                    if len(parts) == 2 and parts[1].isascii() and parts[1].isdigit():
                        entries.append((int(parts[1]), order, f"{parts[0]} {int(parts[1])}"))
        except FileNotFoundError:
            entries = []
        except OSError:
            self.send_error(500)
            return
        entries.sort(key=lambda entry: -entry[0])
        body = "\n".join(entry[2] for entry in entries[:10]).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        pass


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8765"))
    server = ThreadingHTTPServer(("127.0.0.1", port), ScoreHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

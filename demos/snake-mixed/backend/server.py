#!/usr/bin/env python3
"""Small standard-library HTTP backend for the snake game's scores."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path


SCORES_FILE = Path(__file__).resolve().with_name("scores.txt")


class ScoreHandler(BaseHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def send_error(self, code, message=None, explain=None):
        """Return errors as plain text while keeping the CORS header."""
        reason = message or self.responses.get(code, ("Unknown error",))[0]
        body = f"{code} {reason}\n".encode("utf-8")
        self.send_response(code, reason)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_GET(self):
        if self.path != "/scores":
            self.send_error(404)
            return

        scores = []
        try:
            with SCORES_FILE.open("r", encoding="utf-8") as score_file:
                for line in score_file:
                    parts = line.split()
                    if len(parts) == 2:
                        try:
                            scores.append((int(parts[1]), line.rstrip("\r\n")))
                        except ValueError:
                            continue
        except FileNotFoundError:
            pass

        scores.sort(key=lambda item: item[0], reverse=True)
        body = "\n".join(line for _, line in scores[:10]).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path != "/scores":
            self.send_error(404)
            return

        try:
            length = int(self.headers.get("Content-Length", ""))
            if length <= 0:
                raise ValueError
            body = self.rfile.read(length).decode("utf-8")
            parts = body.split()
            if len(parts) != 2 or any(char.isspace() for char in parts[0]):
                raise ValueError
            name, raw_score = parts
            score = int(raw_score)
        except (ValueError, UnicodeDecodeError):
            self.send_error(400)
            return

        with SCORES_FILE.open("a", encoding="utf-8") as score_file:
            score_file.write(f"{name} {score}\n")
        self.send_response(204)
        self.end_headers()


def main():
    port = int(os.environ.get("PORT", "8765"))
    server = ThreadingHTTPServer(("127.0.0.1", port), ScoreHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

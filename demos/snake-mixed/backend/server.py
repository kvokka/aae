#!/usr/bin/env python3
"""Small standard-library HTTP service for snake high scores."""

from http.server import BaseHTTPRequestHandler, HTTPServer
import os
import re


HERE = os.path.dirname(os.path.abspath(__file__))
SCORES_FILE = os.path.join(HERE, "scores.txt")


class ScoreHandler(BaseHTTPRequestHandler):
    def send_response(self, code, message=None):
        super().send_response(code, message)
        self.send_header("Access-Control-Allow-Origin", "*")

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_GET(self):
        if self.path != "/scores":
            self.send_error(404)
            return

        scores = []
        try:
            with open(SCORES_FILE, "r", encoding="utf-8") as score_file:
                for line in score_file:
                    parts = line.split()
                    if len(parts) == 2 and re.fullmatch(r"\d+", parts[1]):
                        scores.append((int(parts[1]), parts[0]))
        except FileNotFoundError:
            pass

        scores.sort(key=lambda item: item[0], reverse=True)
        body = "".join("{} {}\n".format(name, score) for score, name in scores[:10]).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_POST(self):
        if self.path != "/scores":
            self.send_error(404)
            return

        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            body = self.rfile.read(length).decode("utf-8")
        except (ValueError, UnicodeDecodeError):
            self.send_error(400)
            return

        parts = body.split()
        if len(parts) != 2 or not parts[0] or not re.fullmatch(r"\d+", parts[1]):
            self.send_error(400)
            return

        try:
            with open(SCORES_FILE, "a", encoding="utf-8") as score_file:
                score_file.write("{} {}\n".format(parts[0], int(parts[1])))
        except OSError:
            self.send_error(500)
            return

        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, fmt, *args):
        # Keep routine request logs out of the terminal.
        pass


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8765"))
    HTTPServer(("127.0.0.1", port), ScoreHandler).serve_forever()

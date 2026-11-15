#!/usr/bin/env python3
"""Small standard-library HTTP backend for the browser snake game."""

import os
import re
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path


SCORES_FILE = Path(__file__).resolve().parent / "scores.txt"
SCORE_BODY = re.compile(r"\s*([^\s]+)\s+([+-]?\d+)\s*\Z")


class SnakeHandler(BaseHTTPRequestHandler):
    def _send(self, status, body=b"", content_type=None, extra_headers=()):
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        if content_type:
            self.send_header("Content-Type", content_type)
        for name, value in extra_headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_OPTIONS(self):
        self._send(
            204,
            extra_headers=(
                ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
                ("Access-Control-Allow-Headers", "Content-Type"),
            ),
        )

    def do_POST(self):
        if self.path != "/scores":
            self._send(404)
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise ValueError
            body = raw.decode("utf-8")
        except (ValueError, UnicodeDecodeError):
            self._send(400)
            return

        match = SCORE_BODY.fullmatch(body)
        if not match:
            self._send(400)
            return
        name, score_text = match.groups()
        try:
            score = int(score_text)
        except ValueError:
            self._send(400)
            return

        with SCORES_FILE.open("a", encoding="utf-8") as scores:
            scores.write(f"{name} {score}\n")
        self._send(204)

    def do_GET(self):
        if self.path != "/scores":
            self._send(404)
            return

        entries = []
        try:
            with SCORES_FILE.open("r", encoding="utf-8") as scores:
                for line in scores:
                    fields = line.split()
                    if len(fields) != 2:
                        continue
                    try:
                        entries.append((int(fields[1]), line.rstrip("\r\n")))
                    except ValueError:
                        continue
        except FileNotFoundError:
            pass

        entries.sort(key=lambda entry: entry[0], reverse=True)
        response = "".join(line + "\n" for _, line in entries[:10]).encode("utf-8")
        self._send(200, response, "text/plain; charset=utf-8")

    def log_message(self, format, *args):
        pass


def main():
    port = int(os.environ.get("PORT", "8765"))
    HTTPServer(("127.0.0.1", port), SnakeHandler).serve_forever()


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Small persistent HTTP API for snake high scores."""

import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from tempfile import NamedTemporaryFile


PORT = 8000
TOP_COUNT = 10
DEFAULT_STORE = Path(__file__).with_name("scores.json")
STORE_PATH = Path(os.environ.get("SCORE_STORE_PATH", DEFAULT_STORE))
ALLOWED_ORIGINS = {"http://127.0.0.1:5173", "http://localhost:5173"}


def load_scores():
    try:
        with STORE_PATH.open(encoding="utf-8") as store:
            scores = json.load(store)
        if not isinstance(scores, list):
            return []
        return scores[:TOP_COUNT]
    except FileNotFoundError:
        return []


def save_scores(scores):
    """Replace the store atomically so an interrupted write cannot truncate it."""
    STORE_PATH.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with NamedTemporaryFile("w", encoding="utf-8", dir=STORE_PATH.parent,
                               delete=False) as store:
            temporary = store.name
            json.dump(scores, store, ensure_ascii=False)
            store.write("\n")
            store.flush()
            os.fsync(store.fileno())
        os.replace(temporary, STORE_PATH)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


class ScoresHandler(BaseHTTPRequestHandler):
    def _send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        origin = self.headers.get("Origin")
        if origin in ALLOWED_ORIGINS:
            self.send_header("Access-Control-Allow-Origin", origin)
            self.send_header("Vary", "Origin")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        origin = self.headers.get("Origin")
        self.send_response(204)
        if origin in ALLOWED_ORIGINS:
            self.send_header("Access-Control-Allow-Origin", origin)
            self.send_header("Vary", "Origin")
            self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path != "/api/scores":
            self._send_json(404, {"error": "Not found"})
            return
        scores = sorted(load_scores(), key=lambda entry: entry["score"], reverse=True)
        self._send_json(200, {"scores": scores})

    def do_POST(self):
        if self.path != "/api/scores":
            self._send_json(404, {"error": "Not found"})
            return
        if self.headers.get_content_type() != "application/json":
            self._send_json(400, {"error": "Content-Type must be application/json"})
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            payload = json.loads(self.rfile.read(length))
        except (ValueError, json.JSONDecodeError, UnicodeDecodeError):
            self._send_json(400, {"error": "Request body must be valid JSON"})
            return
        if not isinstance(payload, dict):
            self._send_json(400, {"error": "Request body must be a JSON object"})
            return
        name = payload.get("name")
        score = payload.get("score")
        if not isinstance(name, str):
            self._send_json(400, {"error": "Name must be a string"})
            return
        name = name.strip()
        if not 1 <= len(name) <= 20:
            self._send_json(400, {"error": "Name must be 1 to 20 characters after trimming"})
            return
        if isinstance(score, bool) or not isinstance(score, int) or score < 0:
            self._send_json(400, {"error": "Score must be a non-negative integer"})
            return

        scores = load_scores()
        scores.append({"name": name, "score": score})
        scores = sorted(scores, key=lambda entry: entry["score"], reverse=True)[:TOP_COUNT]
        save_scores(scores)
        self._send_json(201, {"scores": scores})

    def log_message(self, format, *args):
        print("%s - %s" % (self.address_string(), format % args))


if __name__ == "__main__":
    print("High-score API listening on http://127.0.0.1:%d" % PORT, flush=True)
    HTTPServer(("127.0.0.1", PORT), ScoresHandler).serve_forever()

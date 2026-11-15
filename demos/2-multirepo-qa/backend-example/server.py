#!/usr/bin/env python3
"""Small persistent high score API for the Snake game."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading


HOST = "localhost"
PORT = 3000
STORE = Path(__file__).resolve().parent / "scores.json"
store_lock = threading.RLock()


def load_scores():
    """Load the score list, creating the empty store on first use."""
    with store_lock:
        if not STORE.exists():
            save_scores([])
        try:
            data = json.loads(STORE.read_text(encoding="utf-8"))
            if not isinstance(data, list):
                raise ValueError("score store must contain a JSON list")
            return data
        except (OSError, json.JSONDecodeError, ValueError) as exc:
            raise RuntimeError("Could not read score store: " + str(exc)) from exc


def save_scores(scores):
    """Atomically replace the JSON store so interrupted writes do not truncate it."""
    STORE.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(prefix="scores-", suffix=".tmp", dir=STORE.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as temp_file:
            json.dump(scores, temp_file, ensure_ascii=False, indent=2)
            temp_file.write("\n")
            temp_file.flush()
            os.fsync(temp_file.fileno())
        os.replace(temp_name, STORE)
    finally:
        if os.path.exists(temp_name):
            os.unlink(temp_name)


class ScoreHandler(BaseHTTPRequestHandler):
    def send_json(self, status, payload):
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "http://localhost:5173")
        self.send_header("Vary", "Origin")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        if self.path != "/api/scores":
            self.send_json(404, {"error": "Not found"})
            return
        self.send_json(204, {})

    def do_GET(self):
        if self.path != "/api/scores":
            self.send_json(404, {"error": "Not found"})
            return
        try:
            with store_lock:
                scores = load_scores()
            self.send_json(200, {"scores": scores[:10]})
        except RuntimeError as exc:
            self.send_json(500, {"error": str(exc)})

    def do_POST(self):
        if self.path != "/api/scores":
            self.send_json(404, {"error": "Not found"})
            return
        if self.headers.get_content_type() != "application/json":
            self.send_json(400, {"error": "Content-Type must be application/json"})
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            payload = json.loads(self.rfile.read(length).decode("utf-8"))
        except (ValueError, UnicodeDecodeError, json.JSONDecodeError):
            self.send_json(400, {"error": "Request body must be valid JSON"})
            return
        if not isinstance(payload, dict):
            self.send_json(400, {"error": "Request body must be a JSON object"})
            return
        name = payload.get("name")
        score = payload.get("score")
        if not isinstance(name, str):
            self.send_json(400, {"error": "Name must be a string"})
            return
        name = name.strip()
        if not 1 <= len(name) <= 24:
            self.send_json(400, {"error": "Name must be 1 to 24 characters after trimming"})
            return
        if isinstance(score, bool) or not isinstance(score, int) or score < 0:
            self.send_json(400, {"error": "Score must be a nonnegative integer"})
            return

        try:
            with store_lock:
                scores = load_scores()
                scores.append({"name": name, "score": score})
                # Python's sort is stable, so earlier submissions stay first on ties.
                scores.sort(key=lambda entry: entry["score"], reverse=True)
                scores = scores[:10]
                save_scores(scores)
            self.send_json(201, {"scores": scores})
        except (RuntimeError, OSError, KeyError, TypeError) as exc:
            self.send_json(500, {"error": "Could not save score: " + str(exc)})

    def log_message(self, fmt, *args):
        print("%s - %s" % (self.address_string(), fmt % args))


if __name__ == "__main__":
    # Ensure first-use store creation before announcing the server is ready.
    load_scores()
    print("Snake score API listening on http://%s:%d" % (HOST, PORT), flush=True)
    ThreadingHTTPServer((HOST, PORT), ScoreHandler).serve_forever()

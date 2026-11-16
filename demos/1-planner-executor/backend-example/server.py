#!/usr/bin/env python3
"""Small JSON API for a browser Snake game's high scores."""

import json
import os
import tempfile
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


SCORES_FILE = os.environ.get("SCORES_FILE", "./scores.json")
_write_lock = threading.RLock()


def read_scores(path=SCORES_FILE):
    try:
        with open(path, "r", encoding="utf-8") as scores_file:
            content = scores_file.read()
        return json.loads(content) if content.strip() else []
    except FileNotFoundError:
        return []


def save_scores(records, path=SCORES_FILE):
    directory = os.path.dirname(os.path.abspath(path))
    temporary_path = None
    with _write_lock:
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8", dir=directory, delete=False
            ) as temporary_file:
                temporary_path = temporary_file.name
                json.dump(records, temporary_file)
                temporary_file.write("\n")
            os.replace(temporary_path, path)
        finally:
            if temporary_path and os.path.exists(temporary_path):
                os.unlink(temporary_path)


def create_handler(scores_path=SCORES_FILE):
    class ScoresHandler(BaseHTTPRequestHandler):
        def send_json(self, status, value):
            body = json.dumps(value).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self._cors_headers()
            self.end_headers()
            self.wfile.write(body)

        def _cors_headers(self):
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            self.send_header("Access-Control-Allow-Headers", "Content-Type")

        def do_OPTIONS(self):
            self.send_response(204)
            self._cors_headers()
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_GET(self):
            if self.path != "/api/scores":
                self.send_json(404, {"error": "not found"})
                return
            records = read_scores(scores_path)
            records.sort(key=lambda record: (-record["score"], record["created_at"]))
            self.send_json(200, records[:10])

        def do_POST(self):
            if self.path != "/api/scores":
                self.send_json(404, {"error": "not found"})
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                payload = json.loads(self.rfile.read(length))
            except (ValueError, json.JSONDecodeError, UnicodeDecodeError):
                self.send_json(400, {"error": "invalid JSON"})
                return
            if not isinstance(payload, dict):
                self.send_json(400, {"error": "body must be a JSON object"})
                return
            name = payload.get("name")
            if not isinstance(name, str):
                self.send_json(400, {"error": "name must be a string"})
                return
            name = name.strip()
            if not 1 <= len(name) <= 20:
                self.send_json(400, {"error": "name must be 1 to 20 characters"})
                return
            score = payload.get("score")
            if isinstance(score, bool) or not isinstance(score, int) or score < 0:
                self.send_json(400, {"error": "score must be a non-negative integer"})
                return

            record = {
                "name": name,
                "score": score,
                "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            }
            with _write_lock:
                records = read_scores(scores_path)
                records.append(record)
                save_scores(records, scores_path)
            self.send_json(201, record)

    return ScoresHandler


def main():
    port = int(os.environ.get("PORT", "8000"))
    server = ThreadingHTTPServer(("127.0.0.1", port), create_handler())
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

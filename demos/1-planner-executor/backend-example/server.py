"""A small SQLite-backed high-score API for a browser Snake game."""

import json
import os
import sqlite3
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path


BASE_DIR = Path(__file__).resolve().parent
DATABASE_PATH = BASE_DIR / "scores.db"


def initialize_database():
    with sqlite3.connect(DATABASE_PATH) as connection:
        connection.execute(
            """CREATE TABLE IF NOT EXISTS scores (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                score INTEGER NOT NULL,
                created_at TEXT NOT NULL
            )"""
        )


class ScoresHandler(BaseHTTPRequestHandler):
    def send_json(self, status, payload=None):
        body = b"" if payload is None else json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_OPTIONS(self):
        if self.path.startswith("/api/scores"):
            self.send_json(204)
        else:
            self.send_json(404, {"error": "not found"})

    def do_GET(self):
        if self.path != "/api/scores":
            self.send_json(404, {"error": "not found"})
            return

        with sqlite3.connect(DATABASE_PATH) as connection:
            rows = connection.execute(
                "SELECT name, score FROM scores "
                "ORDER BY score DESC, created_at ASC, id ASC LIMIT 10"
            ).fetchall()
        self.send_json(200, [{"name": name, "score": score} for name, score in rows])

    def do_POST(self):
        if self.path != "/api/scores":
            self.send_json(404, {"error": "not found"})
            return

        if self.headers.get_content_type() != "application/json":
            self.send_json(400, {"error": "Content-Type must be application/json"})
            return

        try:
            length = int(self.headers.get("Content-Length", "0"))
            payload = json.loads(self.rfile.read(length))
        except (ValueError, json.JSONDecodeError, UnicodeDecodeError):
            self.send_json(400, {"error": "invalid JSON"})
            return

        if not isinstance(payload, dict):
            self.send_json(400, {"error": "request body must be a JSON object"})
            return

        name = payload.get("name")
        score = payload.get("score")
        if not isinstance(name, str) or not name.strip():
            self.send_json(400, {"error": "name must be a non-empty string"})
            return
        name = name.strip()
        if len(name) > 20:
            self.send_json(400, {"error": "name must be at most 20 characters"})
            return
        if isinstance(score, bool) or not isinstance(score, int) or score < 0:
            self.send_json(400, {"error": "score must be a non-negative integer"})
            return

        created_at = datetime.now(timezone.utc).isoformat()
        with sqlite3.connect(DATABASE_PATH) as connection:
            connection.execute(
                "INSERT INTO scores (name, score, created_at) VALUES (?, ?, ?)",
                (name, score, created_at),
            )
        self.send_json(201, {"name": name, "score": score})

    def log_message(self, format_string, *args):
        print("%s - %s" % (self.address_string(), format_string % args))


def main():
    initialize_database()
    port = int(os.environ.get("PORT", "8000"))
    server = HTTPServer(("127.0.0.1", port), ScoresHandler)
    print("Snake high-score API listening on http://127.0.0.1:%d" % port)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

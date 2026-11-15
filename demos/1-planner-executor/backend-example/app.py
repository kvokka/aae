"""Small SQLite-backed high-score API for the Snake game."""

import json
import os
import sqlite3
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


DB_PATH = Path(__file__).with_name("scores.db")


def initialize_database(db_path):
    with sqlite3.connect(db_path) as connection:
        connection.execute(
            """CREATE TABLE IF NOT EXISTS scores (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                score INTEGER NOT NULL,
                created_at TEXT NOT NULL
            )"""
        )


class ScoreHandler(BaseHTTPRequestHandler):
    def send_json(self, status, value=None):
        body = b"" if value is None else json.dumps(value).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_json(204)

    def do_GET(self):
        parsed = urlsplit(self.path)
        if parsed.path != "/api/scores":
            self.send_json(404, {"error": "not found"})
            return

        params = parse_qs(parsed.query, keep_blank_values=True)
        values = params.get("limit", ["10"])
        try:
            if len(values) != 1:
                raise ValueError
            limit = int(values[0])
            if not 1 <= limit <= 100:
                raise ValueError
        except ValueError:
            self.send_json(400, {"error": "limit must be an integer from 1 to 100"})
            return

        with sqlite3.connect(self.server.db_path) as connection:
            connection.row_factory = sqlite3.Row
            rows = connection.execute(
                "SELECT id, name, score, created_at FROM scores "
                "ORDER BY score DESC, created_at ASC LIMIT ?",
                (limit,),
            ).fetchall()
        self.send_json(200, [dict(row) for row in rows])

    def do_POST(self):
        if urlsplit(self.path).path != "/api/scores":
            self.send_json(404, {"error": "not found"})
            return

        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            body = json.loads(self.rfile.read(length))
            if not isinstance(body, dict):
                raise ValueError
            name, score = body["name"], body["score"]
            if not isinstance(name, str):
                raise ValueError
            name = name.strip()
            if not 1 <= len(name) <= 20:
                raise ValueError
            if isinstance(score, bool) or not isinstance(score, int) or not 0 <= score <= 1_000_000:
                raise ValueError
        except (ValueError, TypeError, KeyError, UnicodeError):
            self.send_json(400, {"error": "body must contain a name (1-20 characters) and score (0-1000000 integer)"})
            return

        created_at = datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")
        with sqlite3.connect(self.server.db_path) as connection:
            cursor = connection.execute(
                "INSERT INTO scores (name, score, created_at) VALUES (?, ?, ?)",
                (name, score, created_at),
            )
            saved = {"id": cursor.lastrowid, "name": name, "score": score, "created_at": created_at}
        self.send_json(201, saved)

    def log_message(self, format, *args):
        pass


def create_server(db_path=DB_PATH, host="0.0.0.0", port=8000):
    initialize_database(db_path)
    server = ThreadingHTTPServer((host, port), ScoreHandler)
    server.db_path = str(db_path)
    return server


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8000"))
    server = create_server(port=port)
    print(f"Snake high-score API listening on 0.0.0.0:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

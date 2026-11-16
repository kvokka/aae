#!/usr/bin/env python3
"""Persistent, deliberately small high-score API for the Snake game."""

import json
import os
import sqlite3
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

HOST = "127.0.0.1"
PORT = 8000
DATABASE_PATH = Path(__file__).with_name("highscores.sqlite3")
ALLOWED_ORIGINS = {"http://127.0.0.1:5173", "http://localhost:5173"}
API_PATH = "/api/highscores"


def connect_database(path=None):
    if path is None:
        path = DATABASE_PATH
    connection = sqlite3.connect(path)
    connection.execute(
        """CREATE TABLE IF NOT EXISTS scores (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               name TEXT NOT NULL,
               score INTEGER NOT NULL
           )"""
    )
    return connection


class HighScoreHandler(BaseHTTPRequestHandler):
    def _cors_headers(self):
        origin = self.headers.get("Origin")
        if origin in ALLOWED_ORIGINS:
            self.send_header("Access-Control-Allow-Origin", origin)
            self.send_header("Vary", "Origin")

    def _json(self, status, payload):
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self._cors_headers()
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        if self.path != API_PATH:
            self._json(404, {"error": "Not found"})
            return
        self.send_response(204)
        self._cors_headers()
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Access-Control-Max-Age", "600")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path != API_PATH:
            self._json(404, {"error": "Not found"})
            return
        try:
            with connect_database() as connection:
                rows = connection.execute(
                    "SELECT name, score FROM scores ORDER BY score DESC, id ASC LIMIT 10"
                ).fetchall()
            self._json(200, {"scores": [{"name": name, "score": score} for name, score in rows]})
        except sqlite3.Error:
            self._json(500, {"error": "Could not read scores"})

    def do_POST(self):
        if self.path != API_PATH:
            self._json(404, {"error": "Not found"})
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0 or length > 4096:
                raise ValueError
            payload = json.loads(self.rfile.read(length).decode("utf-8"))
            if not isinstance(payload, dict):
                raise ValueError("Request body must be a JSON object")
            name = payload.get("name")
            score = payload.get("score")
            if not isinstance(name, str):
                raise ValueError("Name must be a string")
            name = name.strip()
            if not 1 <= len(name) <= 24:
                raise ValueError("Name must be 1 to 24 characters")
            if isinstance(score, bool) or not isinstance(score, int):
                raise ValueError("Score must be an integer")
            if not 0 <= score <= 2147483647:
                raise ValueError("Score must be between 0 and 2147483647")
        except (ValueError, UnicodeDecodeError, json.JSONDecodeError):
            self._json(400, {"error": "Invalid JSON request or field values"})
            return

        try:
            with connect_database() as connection:
                connection.execute("INSERT INTO scores (name, score) VALUES (?, ?)", (name, score))
            self._json(201, {"name": name, "score": score})
        except sqlite3.Error:
            self._json(500, {"error": "Could not save score"})

    def _method_not_allowed(self):
        self.send_response(405)
        self._cors_headers()
        self.send_header("Allow", "GET, POST, OPTIONS")
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", "0")
        self.end_headers()

    do_PUT = _method_not_allowed
    do_DELETE = _method_not_allowed
    do_PATCH = _method_not_allowed

    def log_message(self, format, *args):
        print("%s - %s" % (self.address_string(), format % args))


if __name__ == "__main__":
    # Allow tests/operators to choose a temporary location; the default stays
    # beside this script and is excluded from version control.
    DATABASE_PATH = Path(os.environ.get("SNAKE_HIGHSCORES_DB", DATABASE_PATH))
    print("High-score API listening on http://%s:%d" % (HOST, PORT))
    HTTPServer((HOST, PORT), HighScoreHandler).serve_forever()

"""Small standard-library HTTP API for Snake high scores."""

import json
import os
import tempfile
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


SCORES_FILE = os.environ.get("SCORES_FILE", "scores.json")
scores_lock = threading.Lock()


def read_scores():
    with open(SCORES_FILE, "r", encoding="utf-8") as scores_file:
        return json.load(scores_file)


def write_scores(entries):
    directory = os.path.dirname(os.path.abspath(SCORES_FILE))
    descriptor, temporary_path = tempfile.mkstemp(dir=directory, prefix=".scores-")
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as temporary_file:
            json.dump(entries, temporary_file, ensure_ascii=False)
            temporary_file.write("\n")
        os.replace(temporary_path, SCORES_FILE)
    finally:
        if os.path.exists(temporary_path):
            os.unlink(temporary_path)


class ScoreHandler(BaseHTTPRequestHandler):
    def send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def send_error_json(self, status, message):
        self.send_json(status, {"error": message})

    def do_OPTIONS(self):
        if urlsplit(self.path).path != "/api/scores":
            self.send_error_json(404, "Not found")
            return
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        parsed = urlsplit(self.path)
        if parsed.path != "/api/scores":
            self.send_error_json(404, "Not found")
            return
        parameters = parse_qs(parsed.query, keep_blank_values=True)
        values = parameters.get("limit", ["10"])
        if len(values) != 1 or not values[0].isdigit():
            self.send_error_json(400, "limit must be an integer from 1 to 100")
            return
        limit = int(values[0])
        if not 1 <= limit <= 100:
            self.send_error_json(400, "limit must be an integer from 1 to 100")
            return
        with scores_lock:
            entries = read_scores()
        entries.sort(key=lambda entry: (-entry["score"], entry["created_at"]))
        self.send_json(200, entries[:limit])

    def do_POST(self):
        if urlsplit(self.path).path != "/api/scores":
            self.send_error_json(404, "Not found")
            return
        if self.headers.get_content_type() != "application/json":
            self.send_error_json(400, "Content-Type must be application/json")
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
            if length < 0:
                raise ValueError
            payload = json.loads(self.rfile.read(length))
        except (ValueError, UnicodeDecodeError, json.JSONDecodeError):
            self.send_error_json(400, "Request body must be valid JSON")
            return
        if not isinstance(payload, dict):
            self.send_error_json(400, "Body must be a JSON object")
            return
        name = payload.get("name")
        score = payload.get("score")
        if not isinstance(name, str):
            self.send_error_json(400, "name must be a string")
            return
        name = name.strip()
        if not 1 <= len(name) <= 20:
            self.send_error_json(400, "name must contain 1 to 20 characters")
            return
        if isinstance(score, bool) or not isinstance(score, int) or not 0 <= score <= 1000000:
            self.send_error_json(400, "score must be an integer from 0 to 1000000")
            return
        entry = {
            "name": name,
            "score": score,
            "created_at": datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
        }
        with scores_lock:
            entries = read_scores()
            entries.append(entry)
            write_scores(entries)
        self.send_json(201, entry)


if not os.path.exists(SCORES_FILE):
    with scores_lock:
        if not os.path.exists(SCORES_FILE):
            write_scores([])


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8000"))
    server = ThreadingHTTPServer(("127.0.0.1", port), ScoreHandler)
    print(f"Snake score server listening on http://127.0.0.1:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

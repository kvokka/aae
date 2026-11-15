"""HTTP API for Snake high scores."""

import json
import os
import tempfile
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


SCORES_FILE = os.environ.get("SCORES_FILE", "./scores.json")
PORT = int(os.environ.get("PORT", "8000"))
SCORES_LOCK = threading.Lock()


def read_scores():
    """Read all stored scores, returning an empty list before the first write."""
    try:
        with open(SCORES_FILE, "r", encoding="utf-8") as scores_file:
            return json.load(scores_file)
    except FileNotFoundError:
        return []


def save_score(name, score):
    """Append one score and atomically replace the storage file."""
    entry = {
        "name": name,
        "score": score,
        "created_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }
    with SCORES_LOCK:
        scores = read_scores()
        scores.append(entry)
        directory = os.path.dirname(os.path.abspath(SCORES_FILE))
        temporary_path = None
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8", dir=directory, delete=False
            ) as temporary_file:
                temporary_path = temporary_file.name
                json.dump(scores, temporary_file, ensure_ascii=False)
                temporary_file.write("\n")
            os.replace(temporary_path, SCORES_FILE)
        finally:
            if temporary_path and os.path.exists(temporary_path):
                os.unlink(temporary_path)
    return entry


class ScoresHandler(BaseHTTPRequestHandler):
    def _headers(self, status, content_length=None):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        if content_length is not None:
            self.send_header("Content-Length", str(content_length))
        self.end_headers()

    def _json_response(self, status, payload):
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self._headers(status, len(body))
        self.wfile.write(body)

    def _not_found(self):
        self._json_response(404, {"error": "Not found"})

    def _method_not_allowed(self):
        self._json_response(405, {"error": "Method not allowed"})

    def do_OPTIONS(self):
        self._headers(204, 0)

    def do_GET(self):
        if self.path != "/api/scores":
            self._not_found()
            return
        scores = read_scores()
        scores.sort(key=lambda entry: (-entry["score"], entry["created_at"]))
        self._json_response(200, scores[:10])

    def do_POST(self):
        if self.path != "/api/scores":
            self._not_found()
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length)
            data = json.loads(body.decode("utf-8"))
        except (ValueError, UnicodeDecodeError, json.JSONDecodeError):
            self._json_response(400, {"error": "Invalid JSON"})
            return
        if not isinstance(data, dict):
            self._json_response(400, {"error": "Request body must be a JSON object"})
            return

        name = data.get("name")
        if not isinstance(name, str):
            self._json_response(400, {"error": "Name must be a string"})
            return
        name = name.strip()
        if not 1 <= len(name) <= 20:
            self._json_response(400, {"error": "Name must be 1 to 20 characters"})
            return

        score = data.get("score")
        if isinstance(score, bool) or not isinstance(score, int) or not 0 <= score <= 1000000:
            self._json_response(400, {"error": "Score must be an integer from 0 to 1000000"})
            return

        self._json_response(201, save_score(name, score))

    def do_PUT(self):
        self._method_not_allowed() if self.path == "/api/scores" else self._not_found()

    def do_PATCH(self):
        self._method_not_allowed() if self.path == "/api/scores" else self._not_found()

    def do_DELETE(self):
        self._method_not_allowed() if self.path == "/api/scores" else self._not_found()

    def do_HEAD(self):
        self._method_not_allowed() if self.path == "/api/scores" else self._not_found()

    def log_message(self, format_string, *args):
        """Keep request logging in the standard server format."""
        super().log_message(format_string, *args)


def main():
    server = ThreadingHTTPServer(("127.0.0.1", PORT), ScoresHandler)
    print("Snake high-score API listening on http://127.0.0.1:{}".format(PORT))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

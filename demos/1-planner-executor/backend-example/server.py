import json
import os
import tempfile
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer


SCORES_PATH = "scores.json"
API_PATH = "/api/scores"


def load_scores():
    try:
        with open(SCORES_PATH, encoding="utf-8") as scores_file:
            return json.load(scores_file) if scores_file.readable() else []
    except (FileNotFoundError, json.JSONDecodeError):
        return []


def save_scores(scores):
    directory = os.path.dirname(os.path.abspath(SCORES_PATH))
    fd, temporary_path = tempfile.mkstemp(dir=directory, prefix=".scores-")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as temporary_file:
            json.dump(scores, temporary_file, ensure_ascii=False)
            temporary_file.write("\n")
        os.replace(temporary_path, SCORES_PATH)
    finally:
        if os.path.exists(temporary_path):
            os.unlink(temporary_path)


class ScoreHandler(BaseHTTPRequestHandler):
    def __getattr__(self, name):
        if name.startswith("do_"):
            return self.unknown_method
        raise AttributeError(name)

    def send_json(self, status, data=None):
        body = b"" if data is None else json.dumps(data, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Length", str(len(body)))
        if status == 405:
            self.send_header("Allow", "GET, POST, OPTIONS")
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_OPTIONS(self):
        if self.path != API_PATH:
            self.send_json(404, {"error": "Not found"})
            return
        self.send_response(204)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path != API_PATH:
            self.send_json(404, {"error": "Not found"})
            return
        self.send_json(200, load_scores())

    def do_POST(self):
        if self.path != API_PATH:
            self.send_json(404, {"error": "Not found"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            payload = json.loads(self.rfile.read(length))
            name = payload["name"].strip()
            score = payload["score"]
            if not isinstance(name, str) or not 1 <= len(name) <= 20:
                raise ValueError
            if isinstance(score, bool) or not isinstance(score, int) or score < 0:
                raise ValueError
        except (ValueError, TypeError, KeyError, AttributeError, json.JSONDecodeError):
            self.send_json(400, {"error": "Invalid name or score"})
            return
        scores = load_scores()
        entry = {"name": name, "score": score,
                 "date": datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")}
        scores.append(entry)
        scores = sorted(scores, key=lambda item: -item["score"])[:10]
        save_scores(scores)
        self.send_json(201, entry)

    def do_PUT(self):
        self.unknown_method()

    def do_PATCH(self):
        self.unknown_method()

    def do_DELETE(self):
        self.unknown_method()

    def do_HEAD(self):
        self.unknown_method()

    def unknown_method(self):
        if self.path != API_PATH:
            self.send_json(404, {"error": "Not found"})
        else:
            self.send_json(405, {"error": "Method not allowed"})

    def log_message(self, format, *args):
        pass


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 8080), ScoreHandler).serve_forever()

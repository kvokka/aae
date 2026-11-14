import os
import re
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


SCORES_FILE = Path(__file__).with_name("scores.txt")
SCORE_RE = re.compile(r"([^\s]+) ([0-9]+)\Z")


class ScoreHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def _send(self, status, body=b"", content_type=None):
        self.send_response(status)
        if content_type is not None:
            self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "GET, POST")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self):
        if self.path != "/scores":
            self._send(404)
            return

        try:
            length = int(self.headers["Content-Length"])
            if length < 0:
                raise ValueError
            body = self.rfile.read(length).decode("utf-8")
        except (ValueError, UnicodeDecodeError):
            self._send(400)
            return

        match = SCORE_RE.fullmatch(body)
        if match is None:
            self._send(400)
            return

        with SCORES_FILE.open("a", encoding="utf-8") as scores_file:
            scores_file.write(body + "\n")
        self._send(204)

    def do_GET(self):
        if self.path != "/scores":
            self._send(404)
            return

        scores = []
        try:
            lines = SCORES_FILE.read_text(encoding="utf-8").splitlines()
        except FileNotFoundError:
            lines = []
        for line in lines:
            match = SCORE_RE.fullmatch(line)
            if match is not None:
                scores.append((int(match.group(2)), line))

        scores.sort(key=lambda item: item[0], reverse=True)
        body = "".join(line + "\n" for _, line in scores[:10]).encode("utf-8")
        self._send(200, body, "text/plain")

    def log_message(self, format, *args):
        pass


def main():
    port = int(os.environ.get("PORT", "8765"))
    server = ThreadingHTTPServer(("127.0.0.1", port), ScoreHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

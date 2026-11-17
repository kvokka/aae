#!/usr/bin/env python3
"""Small standard-library HTTP server for the greeting API."""

import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlsplit


FRONTEND_ORIGIN = "http://localhost:3000"


class GreetingHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        request = urlsplit(self.path)
        if request.path != "/greeting":
            self.send_error(404)
            return

        name = parse_qs(request.query, encoding="utf-8", errors="strict").get(
            "name", [""]
        )[0]
        if not name:
            self.send_error(400, "name is required")
            return

        body = json.dumps({"greeting": f"Hello, {name}!"}, ensure_ascii=False).encode(
            "utf-8"
        )
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", FRONTEND_ORIGIN)
        self.send_header("Vary", "Origin")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", FRONTEND_ORIGIN)
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()


if __name__ == "__main__":
    print("Greeting API listening on http://localhost:8000", flush=True)
    HTTPServer(("0.0.0.0", 8000), GreetingHandler).serve_forever()

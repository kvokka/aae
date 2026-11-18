import http.server, json, sys
LOG = sys.argv[2]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get('content-length', 0)); body = self.rfile.read(n)
        rec = {"path": self.path, "headers": {k: v for k, v in self.headers.items() if k.lower() not in ("x-api-key", "authorization")}}
        try:
            j = json.loads(body); rec["system"] = j.get("system")
        except Exception: rec["raw"] = body[:500].decode(errors="replace")
        with open(LOG, "a") as f: f.write(json.dumps(rec) + "\n")
        out = json.dumps({"type": "error", "error": {"type": "invalid_request_error", "message": "fake"}}).encode()
        self.send_response(400); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(out))); self.end_headers(); self.wfile.write(out)
    do_GET = do_POST
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()

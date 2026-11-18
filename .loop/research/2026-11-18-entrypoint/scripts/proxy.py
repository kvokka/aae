# Logging forward proxy to api.anthropic.com. Logs only rate-limit response headers and the billing system block.
import http.server, json, sys, time, urllib.request, urllib.error
LOG = sys.argv[2]; UP = "https://api.anthropic.com"
HOP = {"host", "content-length", "accept-encoding", "connection", "transfer-encoding"}
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def handle_one(self, method):
        n = int(self.headers.get("content-length", 0)); body = self.rfile.read(n) if n else None
        hdrs = {k: v for k, v in self.headers.items() if k.lower() not in HOP}
        rec = {"t": time.time(), "method": method, "path": self.path, "ua": self.headers.get("user-agent")}
        try:
            j = json.loads(body); s = j.get("system") or []
            rec["billing"] = [b.get("text") for b in s if isinstance(b, dict) and "billing" in b.get("text", "")]
            rec["model"] = j.get("model")
        except Exception: pass
        req = urllib.request.Request(UP + self.path, data=body, headers=hdrs, method=method)
        try: resp = urllib.request.urlopen(req, timeout=600); code = resp.status
        except urllib.error.HTTPError as e: resp = e; code = e.code
        data = resp.read()
        rec["status"] = code
        rec["rl"] = {k: v for k, v in resp.headers.items() if "ratelimit" in k.lower()}
        if "/v1/messages" in self.path:
            with open(LOG, "a") as f: f.write(json.dumps(rec) + "\n")
        self.send_response(code)
        for k, v in resp.headers.items():
            if k.lower() not in ("transfer-encoding", "content-length", "connection", "content-encoding"): self.send_header(k, v)
        self.send_header("content-length", str(len(data))); self.end_headers(); self.wfile.write(data)
    def do_POST(self): self.handle_one("POST")
    def do_GET(self): self.handle_one("GET")
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()

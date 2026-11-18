# Fake Anthropic endpoint returning a valid streamed "OK"; logs a summary of each request body (no credentials).
import http.server, json, sys, time
LOG = sys.argv[2]
def sse(model):
    ev = [("message_start", {"type": "message_start", "message": {"id": "msg_fake", "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 10, "output_tokens": 1}}}),
          ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
          ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "OK"}}),
          ("content_block_stop", {"type": "content_block_stop", "index": 0}),
          ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 2}}),
          ("message_stop", {"type": "message_stop"})]
    return "".join(f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in ev).encode()
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("content-length", 0)); body = self.rfile.read(n)
        rec = {"t": time.time(), "path": self.path}
        try:
            j = json.loads(body)
            sysb = j.get("system") or []
            rec.update(model=j.get("model"), stream=j.get("stream"), max_tokens=j.get("max_tokens"), ntools=len(j.get("tools") or []),
                       system=[(b.get("text") or "")[:160] for b in sysb] if isinstance(sysb, list) else str(sysb)[:300],
                       nmsgs=len(j.get("messages") or []),
                       last=json.dumps(j["messages"][-1])[-400:] if j.get("messages") else None)
        except Exception as e: rec["err"] = str(e)
        with open(LOG, "a") as f: f.write(json.dumps(rec) + "\n")
        if "/v1/messages" in self.path and "count_tokens" not in self.path:
            out = sse(rec.get("model") or "x"); self.send_response(200); self.send_header("content-type", "text/event-stream")
        else:
            out = json.dumps({"input_tokens": 10}).encode(); self.send_response(200); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(out))); self.end_headers(); self.wfile.write(out)
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()

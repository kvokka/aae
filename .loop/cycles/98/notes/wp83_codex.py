"""WP-83 experiment, codex: what stops a command a thread left running after its turn, without touching another
thread of the same app-server. Fake Responses provider, no model.

usage: python3 -I wp83_codex.py <scenario> <workdir>
scenarios: clean | terminate | unsubscribe | interrupt | detached | amp | tree | eof
"""
import http.server
import json
import os
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wp83_proc as P  # noqa: E402

T0 = time.monotonic()


def t():
    return f"{time.monotonic() - T0:6.2f}"


def log(*a):
    print(t(), *a, flush=True)


# ---- fake Responses API: "RUN:<cmd>" in the user message -> exec_command, then text "Started."
class Fake(http.server.BaseHTTPRequestHandler):
    n = 0

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        Fake.n += 1
        n = Fake.n
        inputs = body.get("input", [])
        text = json.dumps(inputs)
        cmd = text.split("RUN:", 1)[1].split("\\n", 1)[0].split('"', 1)[0].replace("\\\\", "\\") if "RUN:" in text else None
        if cmd and not any(i.get("type") == "function_call_output" for i in inputs):
            args = json.dumps({"cmd": cmd, "yield_time_ms": 1000})
            item = {"type": "function_call", "id": f"fc_{n}", "status": "completed", "call_id": f"call_{n}",
                    "name": "exec_command", "arguments": args}
        else:
            item = {"type": "message", "id": f"msg_{n}", "role": "assistant", "status": "completed",
                    "content": [{"type": "output_text", "text": "Started.", "annotations": []}]}
        resp = {"id": f"resp_{n}", "object": "response", "model": body.get("model"), "status": "in_progress",
                "output": []}
        done = dict(resp, status="completed", output=[item],
                    usage={"input_tokens": 10, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 3,
                           "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 13})
        events = [{"type": "response.created", "response": resp},
                  {"type": "response.output_item.added", "output_index": 0, "item": item},
                  {"type": "response.output_item.done", "output_index": 0, "item": item},
                  {"type": "response.completed", "response": done}]
        out = "".join(f"event: {e['type']}\ndata: {json.dumps(dict(e, sequence_number=i))}\n\n"
                      for i, e in enumerate(events)).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(out)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(out)


class Client:
    def __init__(self, proc, logf):
        self.proc, self.logf, self.next, self.wait, self.seen = proc, logf, 0, {}, []
        self.lock = threading.Lock()
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in self.proc.stdout:
            self.logf.write(f"{t()} < {line}")
            self.logf.flush()
            msg = json.loads(line)
            if "id" in msg and "method" not in msg:
                self.wait[msg["id"]] = msg
            else:
                self.seen.append((time.monotonic() - T0, msg))

    def send(self, msg):
        line = json.dumps(msg)
        self.logf.write(f"{t()} > {line}\n")
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def call(self, method, params, timeout=20):
        self.next += 1
        rid = self.next
        self.send({"id": rid, "method": method, "params": params})
        end = time.monotonic() + timeout
        while rid not in self.wait:
            if time.monotonic() > end:
                return {"timeout": True}
            time.sleep(0.01)
        return self.wait.pop(rid)

    def until(self, pred, timeout):
        end = time.monotonic() + timeout
        i = 0
        while time.monotonic() < end:
            while i < len(self.seen):
                if pred(self.seen[i][1]):
                    return self.seen[i]
                i += 1
            time.sleep(0.02)
        return None


def notes_for(c, thread, since):
    return [(round(ts, 2), m["method"], (m["params"].get("item") or {}).get("type") or
             (m["params"].get("status") or {}).get("type") or "")
            for ts, m in c.seen if ts >= since and m.get("params", {}).get("threadId") == thread
            and m["method"] in ("item/completed", "thread/status/changed", "thread/closed", "turn/completed",
                                "turn/started")]


def main():
    scenario, work = sys.argv[1], sys.argv[2]
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fake)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    home = os.path.join(work, "home")
    os.makedirs(home, exist_ok=True)
    with open(os.path.join(home, "config.toml"), "w") as f:
        f.write('model_provider = "fake"\nsandbox_mode = "danger-full-access"\napproval_policy = "never"\n'
                '[model_providers.fake]\nname = "fake"\n'
                f'base_url = "http://127.0.0.1:{srv.server_address[1]}/v1"\nwire_api = "responses"\n'
                'request_max_retries = 0\nstream_max_retries = 0\n')
    env = {"PATH": os.environ["PATH"], "HOME": home, "CODEX_HOME": home, "SHELL": "/bin/sh",
           "AAE_INSTANCE": f"wp83-{scenario}"}
    proc = subprocess.Popen(["codex", "app-server", "-c", "analytics.enabled=false", "-c",
                             "thread_unload_delay_secs=0"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=open(os.path.join(work, "stderr.txt"), "w"), text=True, env=env,
                            start_new_session=True)
    c = Client(proc, open(os.path.join(work, f"rpc-{scenario}.ndjson"), "w"))
    log("app-server pid", proc.pid)
    c.call("initialize", {"clientInfo": {"name": "wp83", "version": "0"}, "capabilities": {"experimentalApi": True}})
    c.send({"method": "initialized"})
    server_cmd = "exec python3 -m http.server --bind 127.0.0.1 0"
    if scenario == "detached":
        server_cmd = "setsid python3 -m http.server --bind 127.0.0.1 0 >/dev/null 2>&1 &"
    if scenario == "tree":  # a shell that stays, with the server as its child
        server_cmd = "python3 -m http.server --bind 127.0.0.1 0; true"
    if scenario == "amp":  # plain shell background, output still on the command's terminal
        server_cmd = "python3 -m http.server --bind 127.0.0.1 0 &"
    threads = {}
    for name, cmd in (("A", server_cmd), ("B", "exec sleep 300")):
        d = os.path.join(work, name)
        os.makedirs(d, exist_ok=True)
        cfg = {"shell_environment_policy": {"set": {"AAE_RUN": f"r{name}", "AAE_ATTEMPT_TOKEN": f"t{name}"}}}
        r = c.call("thread/start", {"cwd": d, "model": "gpt-fake", "config": cfg})
        tid = r["result"]["thread"]["id"]
        threads[name] = tid
        r = c.call("turn/start", {"threadId": tid, "input": [{"type": "text", "text": f"RUN:{cmd}",
                                                                 "text_elements": []}]})
        threads[name + "turn"] = r["result"]["turn"]["id"]
    for name in ("A", "B"):
        got = c.until(lambda m, n=name: m.get("method") == "turn/completed"
                      and m["params"]["threadId"] == threads[n], 30)
        log(f"turn/completed {name}", got and got[1]["params"]["turn"]["status"])
    time.sleep(0.5)
    marks = ["AAE_ATTEMPT_TOKEN", "AAE_INSTANCE"]
    log("tree after both turns:\n" + P.table(marks))
    a = threads["A"]
    lst = c.call("thread/backgroundTerminals/list", {"threadId": a})
    log("backgroundTerminals/list A:", json.dumps(lst.get("result", lst))[:400])
    lstb = c.call("thread/backgroundTerminals/list", {"threadId": threads["B"]})
    log("backgroundTerminals/list B:", json.dumps(lstb.get("result", lstb))[:400])
    since = time.monotonic() - T0
    if scenario in ("clean", "detached", "amp", "tree"):
        log("clean A ->", json.dumps(c.call("thread/backgroundTerminals/clean", {"threadId": a})))
    elif scenario == "terminate":
        pid = lst["result"]["data"][0]["processId"]
        log("terminate A ->", json.dumps(c.call("thread/backgroundTerminals/terminate",
                                                {"threadId": a, "processId": pid})))
    elif scenario == "unsubscribe":
        log("unsubscribe A ->", json.dumps(c.call("thread/unsubscribe", {"threadId": a})))
    elif scenario == "interrupt":
        log("interrupt A ->", json.dumps(c.call("turn/interrupt", {"threadId": a, "turnId": threads["Aturn"]})))
    time.sleep(3)
    log("A notifications after the call:", notes_for(c, a, since))
    log("tree 3 s after:\n" + P.table(marks))
    left_a = P.tagged("AAE_ATTEMPT_TOKEN", "tA")
    log("A processes left:", left_a, "B processes left:", P.tagged("AAE_ATTEMPT_TOKEN", "tB"))
    if left_a and scenario != "eof":
        log("env sweep AAE_ATTEMPT_TOKEN=tA killed", P.sweep("AAE_ATTEMPT_TOKEN", "tA"))
        time.sleep(1)
        log("A notifications after the sweep:", notes_for(c, a, since))
        log("after sweep A:", P.tagged("AAE_ATTEMPT_TOKEN", "tA"), "B:", P.tagged("AAE_ATTEMPT_TOKEN", "tB"))
        log("backgroundTerminals/list A:",
            json.dumps(c.call("thread/backgroundTerminals/list", {"threadId": a}).get("result")))
    if scenario == "unsubscribe":
        log("loaded:", json.dumps(c.call("thread/loaded/list", {}).get("result")))
    # cleanup: EOF, then sweep the instance marker
    proc.stdin.close()
    try:
        proc.wait(10)
    except subprocess.TimeoutExpired:
        proc.kill()
    log("app-server exit", proc.returncode)
    time.sleep(0.5)
    left = P.tagged("AAE_INSTANCE", f"wp83-{scenario}")
    log("left after app-server EOF exit:", left)
    log(P.table(marks))
    P.sweep("AAE_INSTANCE", f"wp83-{scenario}")


if __name__ == "__main__":
    main()

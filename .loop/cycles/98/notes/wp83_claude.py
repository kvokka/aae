"""WP-83 experiment, claude: what stops a background Bash task a session left after its turn. Fake Anthropic
API, no model.

usage: python3 -I wp83_claude.py <scenario> <workdir>
scenarios: stop_task | interrupt | end_session | eof | term | detached
"""
import http.server
import json
import os
import signal
import subprocess
import sys
import threading
import time
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wp83_proc as P  # noqa: E402

T0 = time.monotonic()
CMD = {"detached": ("setsid python3 -m http.server --bind 127.0.0.1 0 >/dev/null 2>&1 &", False)}


def t():
    return f"{time.monotonic() - T0:6.2f}"


def log(*a):
    print(t(), *a, flush=True)


class Fake(http.server.BaseHTTPRequestHandler):
    n = 0
    scenario = ""

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        Fake.n += 1
        n = Fake.n
        msgs = body.get("messages", [])
        has_result = '"tool_result"' in json.dumps(msgs)  # one command per session, then text
        tools = [x.get("name") for x in body.get("tools", [])]
        if "Bash" in tools and not has_result and "RUN:" in json.dumps(msgs):
            cmd, bg = CMD.get(Fake.scenario, ("python3 -m http.server --bind 127.0.0.1 0", True))
            inp = {"command": cmd, "description": "start a server", "run_in_background": bg}
            block = {"type": "tool_use", "id": f"toolu_{n}", "name": "Bash", "input": inp}
            stop = "tool_use"
        else:
            block = {"type": "text", "text": "Started."}
            stop = "end_turn"
        msg = {"id": f"msg_{n}", "type": "message", "role": "assistant", "model": body.get("model"), "content": [],
               "stop_reason": None, "stop_sequence": None,
               "usage": {"input_tokens": 10, "output_tokens": 1, "cache_creation_input_tokens": 0,
                         "cache_read_input_tokens": 0}}
        if not body.get("stream"):
            out = json.dumps(dict(msg, content=[block], stop_reason=stop)).encode()
            ctype = "application/json"
        else:
            empty = dict(block, text="") if block["type"] == "text" else dict(block, input={})
            delta = ({"type": "text_delta", "text": block["text"]} if block["type"] == "text" else
                     {"type": "input_json_delta", "partial_json": json.dumps(block["input"])})
            ev = [("message_start", {"type": "message_start", "message": msg}),
                  ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": empty}),
                  ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": delta}),
                  ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                  ("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None},
                                     "usage": {"output_tokens": 3}}),
                  ("message_stop", {"type": "message_stop"})]
            out = "".join(f"event: {k}\ndata: {json.dumps(v)}\n\n" for k, v in ev).encode()
            ctype = "text/event-stream"
        self.send_response(200)
        self.send_header("content-type", ctype)
        self.send_header("request-id", f"req_{n}")
        self.send_header("content-length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


def main():
    scenario, work = sys.argv[1], sys.argv[2]
    Fake.scenario = scenario
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fake)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    home, rig = os.path.join(work, "home"), os.path.join(work, "rig")
    os.makedirs(home, exist_ok=True)
    os.makedirs(rig, exist_ok=True)
    env = {"PATH": os.environ["PATH"], "HOME": home, "LANG": "C.UTF-8", "SHELL": "/bin/sh",
           "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{srv.server_address[1]}", "ANTHROPIC_API_KEY": "fake",
           "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS": "1",
           "CLAUDE_CODE_DISABLE_CLAUDE_MDS": "1", "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1",
           "AAE_ATTEMPT_TOKEN": "tA", "AAE_INSTANCE": f"wp83c-{scenario}"}
    # A decoy of another session: must survive every mechanism.
    decoy = subprocess.Popen(["sleep", "300"], env=dict(env, AAE_ATTEMPT_TOKEN="tB"), start_new_session=True)
    proc = subprocess.Popen(["claude", "-p", "--input-format", "stream-json", "--output-format", "stream-json",
                             "--verbose", "--setting-sources", "", "--strict-mcp-config", "--model",
                             "claude-haiku-5-5", "--permission-mode", "bypassPermissions",
                             "--session-id", str(uuid.uuid4())],
                            cwd=rig, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=open(os.path.join(work, "stderr.txt"), "w"), text=True, env=env,
                            start_new_session=True)
    log("claude pid", proc.pid)
    logf = open(os.path.join(work, f"stream-{scenario}.ndjson"), "w")
    seen = []

    def read():
        for line in proc.stdout:
            logf.write(f"{t()} {line}")
            logf.flush()
            try:
                seen.append((time.monotonic() - T0, json.loads(line)))
            except ValueError:
                pass
        seen.append((time.monotonic() - T0, {"type": "EOF"}))

    threading.Thread(target=read, daemon=True).start()

    def send(m):
        logf.write(f"{t()} > {json.dumps(m)}\n")
        proc.stdin.write(json.dumps(m) + "\n")
        proc.stdin.flush()

    def until(pred, timeout, start=0):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            for ts, m in seen[start:]:
                if pred(m):
                    return ts, m
            time.sleep(0.05)
        return None

    def brief(m):
        st = m.get("subtype")
        extra = {k: m[k] for k in ("state", "status", "task_id", "tasks", "task_type", "is_backgrounded",
                                   "terminal_reason", "response") if k in m}
        return f"{m.get('type')}/{st} {json.dumps(extra)[:220]}"

    send({"type": "user", "uuid": str(uuid.uuid4()), "session_id": "", "parent_tool_use_id": None,
          "message": {"role": "user", "content": [{"type": "text", "text": "RUN: start the server"}]}})
    if not until(lambda m: m.get("type") == "result", 60):
        log("no result; see stream")
        proc.kill()
        return
    idle = until(lambda m: m.get("subtype") == "session_state_changed" and m.get("state") == "idle", 10)
    log("result; idle at", idle and round(idle[0], 2))
    time.sleep(1)
    marks = ["AAE_ATTEMPT_TOKEN", "AAE_INSTANCE"]
    log("tree after the turn:\n" + P.table(marks))
    tasks = [m for _, m in seen if m.get("subtype") == "background_tasks_changed"]
    log("last background_tasks_changed:", tasks and json.dumps(tasks[-1].get("tasks"))[:300])
    task_id = next((m["task_id"] for _, m in seen if m.get("subtype") == "task_started"), None)
    mark = len(seen)
    since = time.monotonic() - T0
    if scenario == "stop_task":
        send({"type": "control_request", "request_id": "r-stop",
              "request": {"subtype": "stop_task", "task_id": task_id}})
    elif scenario == "interrupt":
        send({"type": "control_request", "request_id": "r-int", "request": {"subtype": "interrupt"}})
    elif scenario == "end_session":
        send({"type": "control_request", "request_id": "r-end", "request": {"subtype": "end_session"}})
    elif scenario == "eof":
        proc.stdin.close()
    elif scenario == "term":
        os.kill(proc.pid, signal.SIGTERM)
    log(f"{scenario} sent at {since:.2f}")
    time.sleep(4)
    for ts, m in seen[mark:]:
        if m.get("type") in ("control_response", "result", "EOF") or m.get("subtype") in (
                "task_notification", "task_updated", "background_tasks_changed", "session_state_changed"):
            log(f"  +{ts - since:5.2f}", brief(m))
    log("claude alive:", proc.poll() is None, "rc:", proc.returncode)
    log("tree 4 s after:\n" + P.table(marks))
    left = [p for p in P.tagged("AAE_ATTEMPT_TOKEN", "tA") if p != proc.pid]
    log("session A processes left besides claude:", left, "decoy alive:", decoy.poll() is None)
    if left and scenario != "eof_wait":
        log("env sweep tA (not claude) killed", [p for p in left if not os.kill(p, signal.SIGKILL)])
        time.sleep(2)
        for ts, m in seen[mark:]:
            if ts > since + 4 and (m.get("subtype") in ("task_notification", "background_tasks_changed",
                                                        "session_state_changed") or m.get("type") == "result"):
                log(f"  +{ts - since:5.2f}", brief(m))
        log("decoy alive:", decoy.poll() is None)
    if proc.poll() is None:
        proc.stdin.close() if not proc.stdin.closed else None
        try:
            proc.wait(10)
        except subprocess.TimeoutExpired:
            proc.kill()
    log("claude rc", proc.returncode, "left:", P.tagged("AAE_INSTANCE", f"wp83c-{scenario}"))
    P.sweep("AAE_INSTANCE", f"wp83c-{scenario}")


if __name__ == "__main__":
    main()

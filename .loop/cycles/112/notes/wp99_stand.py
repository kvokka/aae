"""WP-99 stand, claude: which stream-json lines arrive after the main `result` while a background subagent runs.
Fake Anthropic Messages API, no model.

usage: python3 -I wp99_stand.py <workdir> <run-label> [subagent-sleep-seconds, default 20]
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

T0 = time.monotonic()
AGENT_TOOLS = ("Agent", "Task")
SLEEP = int(sys.argv[3]) if len(sys.argv) > 3 else 20


def t():
    return time.monotonic() - T0


class Fake(http.server.BaseHTTPRequestHandler):
    n = 0
    reqlog = None
    schema_dump = None

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        Fake.n += 1
        n = Fake.n
        msgs = body.get("messages", [])
        dump = json.dumps(msgs)
        tools = {x.get("name"): x for x in body.get("tools", [])}
        agent = next((a for a in AGENT_TOOLS if a in tools), None)
        if agent and Fake.schema_dump and not os.path.exists(Fake.schema_dump):
            json.dump(tools[agent], open(Fake.schema_dump, "w"), indent=1)
        has_result = '"tool_result"' in dump
        sub = "SUB:" in json.dumps(msgs[:1])  # the subagent's first user message is our prompt
        if agent and not sub:  # the main session
            if "task-notification" in dump:
                who, block = "main/notify", {"type": "text", "text": "Noted."}
            elif not has_result:
                inp = {"description": "run the command", "prompt": "SUB: run the command",
                       "subagent_type": "general-purpose", "run_in_background": True}
                who, block = "main/spawn", {"type": "tool_use", "id": f"toolu_{n}", "name": agent, "input": inp}
            else:
                who, block = "main/started", {"type": "text", "text": "Started."}
        elif sub:  # the subagent (general-purpose also gets the Agent tool)
            if not has_result:
                # claude blocks a standalone `sleep N` above some limit; a python sleep is not matched
                cmd = f"sleep {SLEEP}" if SLEEP <= 20 else f"python3 -c 'import time; time.sleep({SLEEP})'"
                inp = {"command": cmd, "description": "sleep"}
                who, block = "sub/bash", {"type": "tool_use", "id": f"toolu_{n}", "name": "Bash", "input": inp}
            else:
                who, block = "sub/done", {"type": "text", "text": "Done."}
        else:
            who, block = "other", {"type": "text", "text": "ok"}
        Fake.reqlog.write(json.dumps({"t": round(t(), 2), "n": n, "who": who, "stream": bool(body.get("stream")),
                                      "model": body.get("model"), "tools": sorted(tools), "msgs": len(msgs),
                                      "sys": json.dumps(body.get("system"))[:160]}) + "\n")
        Fake.reqlog.flush()
        stop = "tool_use" if block["type"] == "tool_use" else "end_turn"
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


def tagged(value):
    """Live pids whose environment carries WP99_TAG=value."""
    found = []
    for p in filter(str.isdigit, os.listdir("/proc")):
        try:
            if f"WP99_TAG={value}".encode() in open(f"/proc/{p}/environ", "rb").read().split(b"\0"):
                if open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()[0] != "Z":
                    found.append(int(p))
        except OSError:
            pass
    return found


def brief(m):
    keys = ("subtype", "state", "status", "task_id", "task_type", "tool_use_id", "parent_tool_use_id", "origin",
            "is_error", "result", "is_backgrounded", "owned_by_subagent", "parent_task_id",
            "last_tool_name", "description", "elapsed_time_seconds", "tool_name")
    out = {k: m[k] for k in keys if k in m}
    if "tasks" in m:
        out["tasks"] = [{k: x.get(k) for k in ("id", "task_id", "type", "task_type", "status", "description")
                         if k in x} for x in m["tasks"]]
    if m.get("type") in ("assistant", "user"):
        out["content"] = [c.get("type") + ":" + (c.get("name") or c.get("text") or "")[:30]
                          for c in m.get("message", {}).get("content", []) if isinstance(c, dict)]
    if m.get("type") == "stream_event":
        out["ev"] = m.get("event", {}).get("type")
    return f"{m.get('type')} {json.dumps(out)[:400]}"


def main():
    work, label = sys.argv[1], sys.argv[2]
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fake)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    home, rig = os.path.join(work, "home"), os.path.join(work, "rig")
    os.makedirs(home, exist_ok=True)
    os.makedirs(rig, exist_ok=True)
    Fake.reqlog = open(os.path.join(work, f"requests-{label}.ndjson"), "w")
    Fake.schema_dump = os.path.join(work, "agent-tool-schema.json")
    env = {"PATH": os.environ["PATH"], "HOME": home, "LANG": "C.UTF-8", "SHELL": "/bin/sh",
           "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{srv.server_address[1]}", "ANTHROPIC_API_KEY": "fake",
           "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS": "1",
           "CLAUDE_CODE_DISABLE_CLAUDE_MDS": "1", "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1", "WP99_TAG": label}
    cmd = ["claude", "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
           "--setting-sources", "", "--strict-mcp-config", "--model", "claude-haiku-5-5",
           "--permission-mode", "bypassPermissions", "--session-id", str(uuid.uuid4())]
    proc = subprocess.Popen(cmd, cwd=rig, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=open(os.path.join(work, f"stderr-{label}.txt"), "w"), text=True, env=env,
                            start_new_session=True)
    raw = open(os.path.join(work, f"stream-{label}.ndjson"), "w")
    seen = []

    def read():
        for line in proc.stdout:
            ts = t()
            raw.write(f"{ts:6.2f} {line}")
            raw.flush()
            try:
                seen.append((ts, json.loads(line)))
            except ValueError:
                seen.append((ts, {"type": "NONJSON", "line": line[:200]}))
        seen.append((t(), {"type": "EOF"}))

    threading.Thread(target=read, daemon=True).start()
    proc.stdin.write(json.dumps({"type": "user", "uuid": str(uuid.uuid4()), "session_id": "",
                                 "parent_tool_use_id": None, "message": {"role": "user", "content": [
                                     {"type": "text", "text": "Start a background agent."}]}}) + "\n")
    proc.stdin.flush()
    # Stop 10 s after the stream goes quiet once the background task list is empty again; hard cap 120 s.
    while t() < 120 + SLEEP:
        time.sleep(0.2)
        if not seen or seen[-1][1].get("type") == "EOF":
            continue
        lists = [m["tasks"] for _, m in seen if m.get("subtype") == "background_tasks_changed"]
        quiet = t() - seen[-1][0]
        if quiet >= 10 and lists and not lists[-1]:
            break
    proc.stdin.close()
    try:
        proc.wait(5)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
    for p in tagged(label):
        os.kill(p, signal.SIGKILL)
    time.sleep(0.5)
    print(f"rc={proc.returncode} left={tagged(label)}")
    for ts, m in seen:
        if m.get("type") == "stream_event":
            continue  # counted below; partial messages are off, so these should not appear
        print(f"{ts:6.2f} {brief(m)}")
    print("stream_event lines:", sum(1 for _, m in seen if m.get("type") == "stream_event"))


if __name__ == "__main__":
    main()

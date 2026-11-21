"""Read-only MCP App for a run of a local loop-manager daemon (stdio)."""

import http.client
import json
import os
import re
import socket
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from pathlib import Path
from urllib.parse import urlencode

URI = "ui://loop-manager/run-card.html"
MIME = "text/html;profile=mcp-app"
TERMINAL = {"passed", "failed", "cancelled"}
PROPERTIES = {
    "run": {"type": "string", "pattern": "^run_[0-9]+$"},
    "socket": {"type": "string", "description": "Unix socket path for a non-default daemon (as in loop-manager --socket)."},
}


def registered_sockets():
    base = Path(os.environ.get("XDG_CONFIG_HOME", ""))
    if not base.is_absolute():
        base = Path.home() / ".config"
    directory = base / "loop-manager/sockets"
    return {link.name: link.resolve() for link in directory.glob("*") if link.is_symlink() and link.is_socket()}


def socket_path(override):
    if override is not None:
        path = Path(override).expanduser()
        if not path.is_absolute():
            raise ValueError("Expected an absolute Unix socket path")
        return path
    if os.environ.get("LOOP_MANAGER_SOCKET"):
        return Path(os.environ["LOOP_MANAGER_SOCKET"])
    for key in ("XDG_RUNTIME_DIR", "XDG_STATE_HOME"):
        base = Path(os.environ.get(key, ""))
        if base.is_absolute():
            return base / "loop-manager/loop-manager.sock"
    return Path.home() / ".local/state/loop-manager/loop-manager.sock"


class Connection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost", timeout=10)
        self.path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(self.timeout)
        self.sock.connect(str(self.path))


def read(path, route):
    with closing(Connection(path)) as connection:
        connection.request("GET", route)
        with connection.getresponse() as response:
            payload = json.load(response)
            if response.status != 200:
                raise ValueError(payload.get("message", response.reason))
            return payload


def changed(path, run, cursor):
    """Wait for a journal change relevant to the card. Return the next safe cursor."""
    deadline = time.monotonic() + 8
    with closing(Connection(path)) as connection:
        connection.request("GET", f"/v1/events?from={cursor + 1}")
        with connection.getresponse() as response:
            if response.status == 410:
                return True, cursor
            if response.status != 200:
                raise ValueError(f"Event stream returned HTTP {response.status}")
            try:
                while time.monotonic() < deadline:
                    line = response.readline()
                    if not line:
                        raise ConnectionError("Event stream closed")
                    if line.startswith((b"event: gap", b"event: journal")):
                        return True, cursor
                    if line.startswith(b"data:"):
                        event = json.loads(line[5:])
                        cursor = event["seq"]
                        if event.get("run") == run or event["type"] == "config.reloaded":
                            return True, cursor
            except TimeoutError:
                pass
    return False, cursor


def snapshot(path, run):
    # Read the watermark first. A transition during either following read is replayed.
    cursor = read(path, "/v1/daemon")["journal_seq"]
    state = read(path, "/v1/runs/" + run)
    query = urlencode({key: state[key] for key in ("project", "formula", "rig")})
    try:
        layout = read(path, "/v1/config/explain?" + query)
    except ValueError:
        layout = {}  # Historical runs can outlive their formula.
    plan = layout.get("steps", [])
    if state["step"] not in {s["step"] for s in plan}:
        plan.append({"step": state["step"], "kind": state["step_kind"]})
    card = {k: state.get(k) for k in (
        "id", "formula", "step", "step_kind", "state", "round", "step_attempt", "agent",
        "back_from", "waiting_for", "reason",
    )}
    card["rounds"] = layout.get("rounds")
    card["gate"] = (state.get("gate") or {}).get("reason")
    card["results"] = {name: item["result"] for name, item in state["steps"].items()}
    summaries = {name: item["summary"] for name, item in state["steps"].items() if item.get("summary")}
    # While entering a step the run can still point at the previous step's attempt.
    outcome = state.get("outcome") or {}
    attempts = state["attempts"]
    if (state["state"] in TERMINAL or state["state"] == "finishing") and outcome.get("summary"):
        if attempts and attempts[-1]["step"] == state["step"] and attempts[-1]["n"] == state["attempt"]:
            summaries[state["step"]] = outcome["summary"]
    card["summaries"] = summaries
    return {"card": card, "plan": plan, "cursor": cursor, "terminal": state["state"] in TERMINAL}


def tool(name, description, watch=False):
    props = dict(PROPERTIES)
    required = ["run"]
    if not watch:
        props["daemon"] = {"type": "string", "description": "Registered daemon name, when the user names a daemon."}
        names = sorted(registered_sockets())
        if names:
            props["daemon"]["enum"] = names
    if watch:
        props["cursor"] = {"type": "integer", "minimum": 0}
        required.append("cursor")
    ui = {"visibility": ["app"]} if watch else {"resourceUri": URI}
    return {"name": name, "description": description, "_meta": {"ui": ui},
            "annotations": {"readOnlyHint": True, "openWorldHint": False},
            "inputSchema": {"type": "object", "properties": props,
                            "required": required, "additionalProperties": False}}


SHOW_NAME = "show_run_card"
WATCH = tool("watch_run_card", "Wait for changes to an open run card.", watch=True)


def call(params):
    name = params["name"]
    args = params.get("arguments", {})
    run = args.get("run")
    if not isinstance(run, str) or not re.fullmatch(r"run_[0-9]+", run):
        raise ValueError("Expected a run ID such as run_1")
    allowed = WATCH["inputSchema"]["properties"] if name == WATCH["name"] else {*PROPERTIES, "daemon"}
    if unknown := args.keys() - allowed:
        raise ValueError(f"Unknown argument: {', '.join(sorted(unknown))}. Open a new card with the daemon socket.")
    path = socket_path(args.get("socket"))
    if name == WATCH["name"]:
        cursor = args["cursor"]
        if type(cursor) is not int or cursor < 0:
            raise ValueError("Expected a non-negative journal cursor")
        refresh, cursor = changed(path, run, cursor)
        if not refresh:
            return {"content": [], "structuredContent": {"cursor": cursor}}
    elif name != SHOW_NAME:
        raise ValueError("Unknown card tool")
    if "daemon" in args:
        if "socket" in args:
            raise ValueError("Choose either daemon or socket")
        daemon = args["daemon"]
        registered = registered_sockets()
        if daemon not in registered:
            raise ValueError(f"Unknown or unavailable daemon: {daemon}")
        path = registered[daemon]
    try:
        data = snapshot(path, run)
    except (FileNotFoundError, ConnectionRefusedError) as error:
        if "daemon" in args or "socket" in args or os.environ.get("LOOP_MANAGER_SOCKET") or name != SHOW_NAME:
            raise ConnectionError(f"Cannot connect to loop-manager at {path}") from error
        registered = registered_sockets()
        alternatives = set(registered.values()) - {path.resolve()}
        if len(alternatives) != 1:
            choices = ", ".join(sorted(registered)) or "an explicit socket path"
            raise ConnectionError(f"Cannot connect to loop-manager at {path}. Choose {choices}.") from error
        path = alternatives.pop()
        data = snapshot(path, run)
    data["socket"] = str(path)
    card = data["card"]
    fields = [run, card["formula"]]
    if card["round"] > 1:
        count = str(card["round"])
        if card["rounds"]:
            count += f"/{card['rounds']}"
        fields.append(f"round {count}")
    summary = " · ".join([*fields, card["step"], card["state"]])
    if card["state"] == "running" and card["step_kind"] == "await":
        summary += " (awaiting a decision)"
    return {"content": [{"type": "text", "text": summary}], "structuredContent": data}


def dispatch(method, params):
    if method == "initialize":
        supported = ("2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25")
        version = params.get("protocolVersion")
        return {"protocolVersion": version if version in supported else supported[-1],
                "capabilities": {"tools": {}, "resources": {}},
                "serverInfo": {"name": "run-card", "version": "0.0.7"}}
    if method in ("ping", "resources/templates/list"):
        return {"resourceTemplates": []} if method.startswith("resources") else {}
    if method == "tools/list":
        show = tool(SHOW_NAME, "Show a live loop-manager run. If the user names a daemon, pass its registered name.")
        return {"tools": [show, WATCH]}
    resource = {"uri": URI, "name": "Run card", "mimeType": MIME}
    if method == "resources/list":
        return {"resources": [resource]}
    if method == "resources/read" and params.get("uri") == URI:
        html = Path(__file__).with_name("card.html").read_text()
        return {"contents": [{**resource, "text": html}]}
    if method == "tools/call":
        try:
            return call(params)
        except (OSError, ValueError, KeyError, TypeError, http.client.HTTPException) as error:
            return {"isError": True, "content": [{"type": "text", "text": str(error)}]}
    raise ValueError(f"Unsupported request: {method}")


def serve():
    lock = threading.Lock()

    def reply(request):
        result = {"jsonrpc": "2.0", "id": request["id"]}
        try:
            result["result"] = dispatch(request["method"], request.get("params", {}))
        except (ValueError, KeyError, OSError) as error:
            result["error"] = {"code": -32601, "message": str(error)}
        with lock:
            print(json.dumps(result), flush=True)

    with ThreadPoolExecutor(max_workers=12) as workers:
        for line in sys.stdin:
            request = json.loads(line)
            if "id" in request:
                workers.submit(reply, request)


if __name__ == "__main__":
    serve()

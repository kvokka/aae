"""Process-tree helpers for the WP-83 experiment: /proc walk, env-marker sweep."""
import os
import signal


def stat(pid):
    try:
        raw = open(f"/proc/{pid}/stat").read()
    except OSError:
        return None
    rest = raw[raw.rindex(")") + 2:].split()
    # fields after comm: state ppid pgrp session ...
    return {"state": rest[0], "ppid": int(rest[1]), "pgid": int(rest[2]), "sid": int(rest[3])}


def cmdline(pid):
    try:
        return open(f"/proc/{pid}/cmdline", "rb").read().replace(b"\0", b" ").decode().strip()
    except OSError:
        return ""


def environ(pid):
    try:
        raw = open(f"/proc/{pid}/environ", "rb").read()
    except OSError:
        return {}
    out = {}
    for kv in raw.split(b"\0"):
        if b"=" in kv:
            k, v = kv.split(b"=", 1)
            out[k.decode()] = v.decode(errors="replace")
    return out


def pids():
    return [int(p) for p in os.listdir("/proc") if p.isdigit()]


def tagged(name, value):
    """Every live process (not a zombie) whose environment has name=value."""
    me = os.getpid()
    found = []
    for pid in pids():
        if pid == me:
            continue
        st = stat(pid)
        if st and st["state"] != "Z" and environ(pid).get(name) == value:
            found.append(pid)
    return found


def table(name_values):
    """Rows for every process carrying any of the given env markers."""
    rows = []
    for pid in sorted(pids()):
        st = stat(pid)
        if not st or st["state"] == "Z":
            continue
        env = environ(pid)
        marks = [f"{k}={env[k]}" for k in name_values if k in env]
        if marks:
            rows.append(f"  pid={pid} ppid={st['ppid']} pgid={st['pgid']} sid={st['sid']} "
                        f"[{' '.join(marks)}] {cmdline(pid)[:90]}")
    return "\n".join(rows) or "  (none)"


def sweep(name, value, sig=signal.SIGKILL):
    killed = []
    for pid in tagged(name, value):
        try:
            os.kill(pid, sig)
            killed.append(pid)
        except ProcessLookupError:
            pass
    return killed

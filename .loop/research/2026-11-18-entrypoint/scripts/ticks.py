# usage: ticks.py <label|none> <model> <nwords> <stop_when_endpoint_7d_ge> <cost_cap_usd> <outdir> <proxy_log>
# Runs identical calls one by one through the proxy; after each, records cost and the 7d/5h utilization from headers.
import json, os, subprocess, sys, time
label, model, nwords, want, cap, out, plog = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), float(sys.argv[5]), sys.argv[6], sys.argv[7]
S = "/tmp/tmp.rQllDKosDj"; CL = "$(mise which claude)"
os.makedirs(out, exist_ok=True)
def last_rl():
    lines = open(plog).read().splitlines()
    r = json.loads(lines[-1]); rl = r["rl"]
    return len(lines), r.get("billing"), rl.get("anthropic-ratelimit-unified-7d-utilization"), rl.get("anthropic-ratelimit-unified-5h-utilization")
env = {"PATH": "/usr/bin:/bin", "HOME": os.environ["HOME"], "ANTHROPIC_BASE_URL": "http://127.0.0.1:18800", "DISABLE_PROMPT_CACHING": "1"}
if label != "none": env["CLAUDE_CODE_ENTRYPOINT"] = label
spent = 0.0; ticks = 0; prev = None; i = 0
log = open(os.path.join(out, "calls.jsonl"), "a")
while spent < cap:
    i += 1; p = os.path.join(out, f"prompt_{i}.txt")
    subprocess.run(["python3", "-I", f"{S}/scripts/mkprompt.py", str(nwords), p], check=True)
    t0 = time.time()
    with open(p) as fin, open(os.path.join(out, f"call_{i}.jsonl"), "w") as fo:
        subprocess.run([CL, "-p", "--output-format", "stream-json", "--verbose", "--model", model, "--tools", "",
                        "--setting-sources", "local", "--strict-mcp-config", "--no-session-persistence", "--effort", "low",
                        "--max-turns", "1"], stdin=fin, stdout=fo, stderr=subprocess.DEVNULL, env=env, cwd=f"{S}/cwd", timeout=600)
    os.remove(p)
    res = {}
    for l in open(os.path.join(out, f"call_{i}.jsonl")):
        try: m = json.loads(l)
        except Exception: continue
        if m.get("type") == "result": res = m
    u = res.get("usage", {}); c = res.get("total_cost_usd", 0.0); spent += c
    n, billing, u7, u5 = last_rl()
    ep = json.loads(subprocess.run(["python3", "-I", f"{S}/scripts/usage.py", os.path.join(out, "usage.jsonl")], capture_output=True, text=True).stdout)["usage"]
    e7 = (ep.get("seven_day") or {}).get("utilization"); e5 = (ep.get("five_hour") or {}).get("utilization") if ep.get("five_hour") else None
    rec = {"i": i, "t0": t0, "t1": time.time(), "label": label, "cost": c, "spent": spent, "in": u.get("input_tokens"),
           "out": u.get("output_tokens"), "cache_read": u.get("cache_read_input_tokens"), "cache_create": u.get("cache_creation_input_tokens"),
           "is_error": res.get("is_error"), "billing": billing, "u7": u7, "u5": u5, "e7": e7, "e5": e5}
    log.write(json.dumps(rec) + "\n"); log.flush(); print(json.dumps(rec), flush=True)
    if e7 is not None and e7 >= want: break

import json, sys, glob
tot = {"cost": 0, "in": 0, "out": 0, "cr": 0, "cc": 0, "calls": 0}; rl = []
for f in sorted(glob.glob(sys.argv[1] + '/call_*.jsonl')):
    for l in open(f):
        try: m = json.loads(l)
        except Exception: continue
        if m.get('type') == 'result':
            u = m.get('usage', {}); tot["calls"] += 1; tot["cost"] += m.get('total_cost_usd', 0)
            tot["in"] += u.get('input_tokens', 0); tot["out"] += u.get('output_tokens', 0)
            tot["cr"] += u.get('cache_read_input_tokens', 0); tot["cc"] += u.get('cache_creation_input_tokens', 0)
            if m.get('is_error'): print('ERR', f, str(m.get('result'))[:200])
        if m.get('type') == 'rate_limit_event': rl.append(m)
print(json.dumps(tot))
for r in rl[-2:]: print(json.dumps(r)[:1500])

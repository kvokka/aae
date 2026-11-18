import json, os, sys, time, urllib.request
d = json.load(open(os.path.expanduser('~/.claude/.credentials.json')))['claudeAiOauth']
req = urllib.request.Request('https://api.anthropic.com/api/oauth/usage',
    headers={'Authorization': 'Bearer ' + d['accessToken'], 'anthropic-beta': 'oauth-2025-04-20',
             'User-Agent': 'claude-cli/2.1.327 (external, cli)', 'Content-Type': 'application/json'})
try:
    body = json.loads(urllib.request.urlopen(req, timeout=20).read())
except urllib.error.HTTPError as e:
    body = {"http_error": e.code, "body": e.read()[:300].decode(errors='replace')}
rec = {"t": time.time(), "iso": time.strftime('%Y-%m-%dT%H:%M:%S'), "usage": body}
line = json.dumps(rec)
if len(sys.argv) > 1:
    open(sys.argv[1], 'a').write(line + "\n")
print(line)

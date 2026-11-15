# snake-mixed: passed

<!-- MEASURE id=demo-snake-mixed date=2026-11-15 ver=aae-0.1.0@96dd606-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 86 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-15 with aae-0.1.0@96dd606-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 86 s from the request.
Tokens the harnesses reported for the runs: input 60251, output 16741.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | codex-coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-mixed.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.files: server.py
data.backend.run: run_2
data.backend.start: PORT=8765 python3 server.py
data.backend.storage: scores.txt next to server.py
data.frontend.backend_url: http://127.0.0.1:8765
data.frontend.files: index.html
data.frontend.run: run_3
id: run_1
reason: pass
state: passed
summary: Snake game with score list. frontend rig: index.html (canvas, arrow keys, name prompt, POST and GET /scores at http://127.0.0.1:8765). backend rig: server.py (stdlib, PORT env default 8765, scores.txt next to it, CORS headers). Verified end to end from a scratch copy: POST 'smoke 4242' gave 204, line in scores.txt, GET /scores returned it with Access-Control-Allow-Origin, OPTIONS gave 204 with Allow-Methods GET, POST, OPTIONS and Allow-Headers Content-Type, bad input gave 400.
```

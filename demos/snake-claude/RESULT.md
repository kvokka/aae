# snake-claude: passed

<!-- MEASURE id=demo-snake-claude date=2026-11-14 ver=aae-0.1.0@b09a833-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 188 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-14 with aae-0.1.0@b09a833-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 188 s from the request.
Tokens the harnesses reported for the runs: input 64, output 33267.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | dev.coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-claude.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.default_port: 8765
data.backend.file: server.py
data.backend.rig: backend
data.backend.scores_file: scores.txt
data.frontend.backend_url: http://127.0.0.1:8765
data.frontend.file: index.html
data.frontend.rig: frontend
id: run_1
reason: pass
state: passed
summary: Snake game built in two rigs: frontend rig has index.html (canvas, arrow keys, POST/GET /scores on http://127.0.0.1:8765, top-10 list) and README.md; backend rig has server.py (stdlib only, 127.0.0.1, PORT or 8765, scores.txt next to server.py, CORS) and README.md. Verified: POST smoke 4242 -> 204, GET lists it, bad body -> 400, OPTIONS -> 204 with CORS headers.
```

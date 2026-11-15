# snake-mixed: passed

<!-- MEASURE id=demo-snake-mixed date=2026-11-15 ver=aae-0.1.0@7fdca97-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 306 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-15 with aae-0.1.0@7fdca97-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 306 s from the request.
Tokens the harnesses reported for the runs: input 75899, output 23776.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | codex-coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-mixed.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.file: server.py
data.backend.port_default: 8765
data.backend.result: passed
data.backend.start: PORT=8765 python3 server.py
data.frontend.backend_url: http://127.0.0.1:8765
data.frontend.file: index.html
data.frontend.result: passed
id: run_1
reason: pass
state: passed
summary: Snake game split across two rigs: backend/server.py (stdlib Python score server on 127.0.0.1, PORT default 8765, scores.txt, CORS) and frontend/index.html (plain-JS canvas snake, arrow keys, name prompt on game over, POST/GET /scores to http://127.0.0.1:8765). Smoke test verified: POST smoke 4242 -> 204, line in scores.txt, GET returns it, OPTIONS -> 204.
```

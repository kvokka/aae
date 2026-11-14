# snake-mixed: passed

<!-- MEASURE id=demo-snake-mixed date=2026-11-14 ver=aae-0.1.0@a992a30,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 110 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-14 with aae-0.1.0@a992a30,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 110 s from the request.
Tokens the harnesses reported for the runs: input 129942, output 21679.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | codex-coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-mixed.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.default_port: 8765
data.backend.file: /tmp/tmp.Ialv4tPRf4/snake-mixed/backend/server.py
data.backend.smoke_test.bad_post_status: 400
data.backend.smoke_test.get_body_after_post: smoke 4242
data.backend.smoke_test.options_status: 204
data.backend.smoke_test.port: 8799
data.backend.smoke_test.valid_post_status: 204
data.frontend.backend_url: http://127.0.0.1:8765/scores
data.frontend.canvas: <canvas id="board">
data.frontend.file: /tmp/tmp.Ialv4tPRf4/snake-mixed/frontend/index.html
data.frontend.verified: node --check and a Node harness with stubbed DOM/fetch; no real browser run
id: run_1
reason: pass
state: passed
summary: Built the snake game in two rigs: backend/server.py (stdlib Python score server on 127.0.0.1, PORT or 8765, scores.txt beside it, POST/GET /scores with CORS and OPTIONS 204) and frontend/index.html (plain JS canvas game, arrow keys, POST and GET to http://127.0.0.1:8765/scores). Both workers passed; the interface (port 8765, paths, formats) matches on both sides. Backend smoke test: posted 'smoke 4242' on port 8799, GET returned it, bad POST gave 400, OPTIONS gave 204.
```

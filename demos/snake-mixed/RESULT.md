# snake-mixed: passed

<!-- MEASURE id=demo-snake-mixed date=2026-11-14 ver=aae-0.1.0@5640682-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 119 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-14 with aae-0.1.0@5640682-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 119 s from the request.
Tokens the harnesses reported for the runs: input 112342, output 26738.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | codex-coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-mixed.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.bad_post_status: 400
data.backend.default_port: 8765
data.backend.deliverable: server.py
data.backend.get_status: 200
data.backend.options_status: 204
data.backend.post_status: 204
data.backend.smoke_test_get_output: smoke 4242
data.frontend.api_base: http://127.0.0.1:8765
data.frontend.arrow_keys: ArrowUp, ArrowDown, ArrowLeft, ArrowRight
data.frontend.deliverable: index.html
data.frontend.uses_canvas: true
id: run_1
reason: pass
state: passed
summary: Built the snake high-score game in two rigs. backend: server.py (stdlib HTTP service on 127.0.0.1, PORT default 8765, scores in scores.txt beside it, POST/GET /scores with CORS, OPTIONS 204). frontend: index.html (single-file plain-JS canvas snake, arrow keys, name prompt, POST to and GET top-ten from http://127.0.0.1:8765/scores). Both runs passed; the POST body format and endpoints match across the two.
```

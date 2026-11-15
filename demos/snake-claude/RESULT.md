# snake-claude: passed

<!-- MEASURE id=demo-snake-claude date=2026-11-15 ver=aae-0.1.0@6b39f7a-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 190 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-15 with aae-0.1.0@6b39f7a-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 190 s from the request.
Tokens the harnesses reported for the runs: input 64, output 28448.

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
data.backend.files: backend/server.py, backend/README.md
data.backend.scores_file: scores.txt next to server.py
data.backend.tests: POST 204 + smoke line in file and GET, 400 on bad input, OPTIONS 204, ordering and top-10 verified
data.frontend.backend_url: http://127.0.0.1:8765/scores
data.frontend.file: frontend/index.html
data.frontend.tests: canvas, arrow keys, game-over post and leaderboard verified in headless Chromium against a stub backend
id: run_1
reason: pass
state: passed
summary: Built the snake game in two rigs: frontend rig has index.html (canvas game, arrow keys, posts 'name score' to http://127.0.0.1:8765/scores, shows top 10); backend rig has server.py (stdlib-only, 127.0.0.1, PORT env default 8765, scores.txt next to it, POST /scores -> 204/400, GET /scores top 10, CORS on every response, OPTIONS 204) plus README.md. Both parts passed their tests.
```

# snake-claude: passed

<!-- MEASURE id=demo-snake-claude date=2026-11-15 ver=aae-0.1.0@7fdca97-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 141 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-15 with aae-0.1.0@7fdca97-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 141 s from the request.
Tokens the harnesses reported for the runs: input 52, output 33815.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | dev.coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-claude.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.cors: Access-Control-Allow-Origin: * on every answer
data.backend.default_port: 8765
data.backend.endpoints: POST /scores (text <name> <score> -> 204 or 400), GET /scores (top 10 text/plain), OPTIONS -> 204
data.backend.files: backend/server.py, backend/scores.txt
data.backend.run: PORT=<port> python3 server.py
data.frontend.backend_url: http://127.0.0.1:8765
data.frontend.files: frontend/index.html
data.frontend.how_to_start: open index.html directly in a browser
data.note: backend/scores.txt currently holds the smoke 4242 line from the worker test
id: run_1
reason: pass
state: passed
summary: Built the snake game and its score backend in two rigs. backend: server.py (stdlib HTTP server on 127.0.0.1, PORT env or 8765, POST/GET /scores, CORS on all answers, OPTIONS 204, scores in scores.txt next to server.py). frontend: index.html (single-file canvas snake, arrow keys, name prompt on game over, posts to and shows the top ten from http://127.0.0.1:8765/scores). Both workers verified their parts; interface checked by reading both files.
```

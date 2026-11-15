# snake-mixed: passed

<!-- MEASURE id=demo-snake-mixed date=2026-11-15 ver=aae-0.1.0@6b39f7a-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 124 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-15 with aae-0.1.0@6b39f7a-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 124 s from the request.
Tokens the harnesses reported for the runs: input 145518, output 21667.

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
data.backend.file: server.py
data.backend.scores_file: scores.txt
data.frontend.api_base: http://127.0.0.1:8765
data.frontend.endpoints: POST /scores, GET /scores
data.frontend.file: index.html
id: run_1
reason: pass
state: passed
summary: Built the snake game and score server. Frontend (rig frontend): index.html, plain JS with a canvas board and arrow-key steering; after a game it asks for a name, POSTs '<name> <score>' to http://127.0.0.1:8765/scores and shows the top ten from GET /scores. Backend (rig backend): server.py, standard library only, listens on 127.0.0.1 at PORT (default 8765), keeps scores in scores.txt next to it, with the CORS headers and OPTIONS 204 as requested. The interfaces match: port, paths and body format agree on both sides.
```

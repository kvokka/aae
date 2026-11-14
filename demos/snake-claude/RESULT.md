# snake-claude: passed

<!-- MEASURE id=demo-snake-claude date=2026-11-14 ver=aae-0.1.0@d5d927b-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 99 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-14 with aae-0.1.0@d5d927b-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 99 s from the request.
Tokens the harnesses reported for the runs: input 50, output 27745.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | dev.coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-claude.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.file: backend/server.py
data.backend.run: PORT=<port> python3 server.py
data.backend.scores_file: backend/scores.txt (created on first POST)
data.backend.verified: POST smoke 4242 -> 204, line in scores.txt, GET lists it, CORS and OPTIONS 204, bad body -> 400
data.frontend.backend_urls: http://127.0.0.1:8765/scores
data.frontend.file: frontend/index.html
data.frontend.verified: canvas, arrow keys, /scores GET and POST present; node --check passed; no browser run
id: run_1
reason: pass
state: passed
summary: Built the snake game in two rigs. frontend rig: index.html, a single plain-JS page with a canvas board, arrow-key steering, a name prompt on game over, POST and GET of /scores to http://127.0.0.1:8765, and a top-ten table. backend rig: server.py, standard library only, listening on 127.0.0.1 on PORT (default 8765), storing scores in scores.txt next to it, with CORS and OPTIONS handling as specified. Interface checked: both sides use http://127.0.0.1:8765/scores and the text/plain '<name> <score>' format. The backend worker verified smoke 4242 end to end; the frontend was checked by reading and a node syntax check only, not in a real browser.
```

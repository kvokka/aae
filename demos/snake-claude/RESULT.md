# snake-claude: passed

<!-- MEASURE id=demo-snake-claude date=2026-11-15 ver=aae-0.1.0@96dd606-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/run.sh note="passed in 144 s from the request, 2 runs started by the planner, n=1" -->

Run on 2026-11-15 with aae-0.1.0@96dd606-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 144 s from the request.
Tokens the harnesses reported for the runs: input 56, output 33271.

The runs, oldest first, with the run that started each:

| run | formula | rig | agent | state | parent |
|---|---|---|---|---|---|
| run_1 | dev.plan | root | dev.planner | passed |  |
| run_2 | dev.code | backend | dev.coder | passed | run_1 |
| run_3 | dev.code | frontend | dev.coder | passed | run_1 |
| run_4 | snake-claude.smoke | root |  | passed | run_1 |

The planner's outcome (`aae run wait`):

```
data.backend.files: server.py, README.md, .gitignore
data.backend.run: PORT=8765 python3 server.py
data.frontend.api: http://127.0.0.1:8765
data.frontend.files: index.html
id: run_1
reason: pass
state: passed
summary: Built the snake game and its score backend. frontend rig: index.html, a single-file plain-JS canvas game (arrow keys, name prompt at game over, POST/GET /scores at http://127.0.0.1:8765, top-ten table). backend rig: server.py (stdlib, 127.0.0.1, PORT default 8765, scores.txt next to it, CORS and OPTIONS as requested), plus README.md and .gitignore. Verified end to end: POST 'smoke 4242' gave 204, the line is in scores.txt and in GET /scores, bad body gave 400, OPTIONS gave 204 with the CORS headers.
```

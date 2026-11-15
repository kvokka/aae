# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-15 ver=aae-0.1.0@c1fa9bc,codex-0.168.0,rumdl-0.3.2,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 1070 s and 2 rounds from the request, n=1" -->

Run on 2026-11-15 with aae-0.1.0@c1fa9bc,codex-0.168.0,rumdl-0.3.2,agent-browser-0.39.0 (`Linux aarch64`): passed in 1070 s from the request,
after 2 round(s) of the checks (at most 3).
This is the second attempt on this code. The first one failed: a coder left a background `http.server` running,
so its turn never went idle without background (spec §7.5) and the run outlived the demo's limit.
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; coder on backend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 2681200, output 30094.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ root-ly9 [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-ly9.1 [● P2] [task] - Build persistent high-score API
├── ✓ root-ly9.2 [● P2] [task] - Build browser Snake and integrate high scores
└── ✓ root-ly9.3 [● P2] [task] - Hide game overlay during active play
```

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.deliver | qa | root | dev.qa | passed |
| run_2 | dev.implement | code | backend | dev.coder | passed |
| run_3 | dev.implement | code | frontend | dev.coder | passed |
| run_4 | dev.implement | code | frontend | dev.coder | passed |

The outcome of the run (`aae run wait`):

```
id: run_1
reason: pass
state: passed
summary: Backend: ran npm start as documented in a temporary copy of the backend rig; after submitting QAPlayer score 0, stopped and restarted it with npm start, and GET /api/high-scores still returned QAPlayer 0. Game: ran the documented Python frontend server in a temporary copy and opened it with headless agent-browser; clicked Start game, took screenshots /tmp/tmp.p0y2U5yy3n/before.png and /tmp/tmp.p0y2U5yy3n/after.png around arrow-key play, and visually confirmed the snake moved with the board unobstructed. Submission and display: the snake hit a wall, the page showed Game over and final score 0, submitting QAPlayer showed Score saved and the leaderboard entry; after backend restart and page reload the entry remained visible. Both servers and the browser were stopped; artifacts and score data remain in the temporary directory.
```

# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-15 ver=aae-0.1.0@4829f4c,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 704 s and 1 rounds from the request, n=1" -->

Run on 2026-11-15 with aae-0.1.0@4829f4c,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 704 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1431997, output 23271.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ root-p7x [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-p7x.1 [● P2] [task] - Build persistent high-score HTTP API
└── ✓ root-p7x.2 [● P2] [task] - Build browser Snake game with high-score integration
```

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.deliver | qa | root | dev.qa | passed |
| run_2 | dev.implement | code | backend | dev.coder | passed |
| run_3 | dev.implement | code | frontend | dev.coder | passed |

The outcome of the run (`aae run wait`):

```
id: run_1
reason: pass
state: passed
summary: Backend: started with python3 server.py using a temporary score store, submitted Persistence QA 12, stopped and restarted it, and GET returned the saved score. Gameplay: served the frontend using its README command, opened it in headless agent-browser, took and inspected before and after screenshots while starting and steering with arrow keys; the snake moved. Scores: let the snake hit a wall, submitted the finished score as Browser QA, saw the success message and both backend scores on the page, and confirmed they remained after reload. Stopped both servers and the browser; artifacts are in /tmp/tmp.UccA4R1TL9.
```

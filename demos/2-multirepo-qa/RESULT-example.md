# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-15 ver=aae-0.1.0@6bbf1f1,codex-0.168.0,rumdl-0.3.2,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 529 s and 1 rounds from the request, n=1" -->

Run on 2026-11-15 with aae-0.1.0@6bbf1f1,codex-0.168.0,rumdl-0.3.2,agent-browser-0.39.0 (`Linux aarch64`): passed in 529 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1164614, output 14976.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ root-2ir [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-2ir.1 [● P2] [task] - Build persistent high-score API
└── ✓ root-2ir.2 [● P2] [task] - Build browser Snake game with high-score leaderboard
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
summary: Backend: started an unchanged copy with the README command python3 server.py, submitted QA Persist score 42, stopped and restarted it, and GET returned the same score. The copy kept test data outside the rig. Game: served the frontend with its README command, opened it headlessly with agent-browser, started and pressed arrow keys; inspected before and after screenshots showing the snake moved and turned. It ate food, scored 1, and hit a wall. Scores: submitted QA Player with the completed score 1, saw the success message and both backend scores on the page, and confirmed both remained visible after reload and in GET. Screenshots and test data are in /tmp/tmp.gKs0yunnAX. Stopped the browser and both servers.
```

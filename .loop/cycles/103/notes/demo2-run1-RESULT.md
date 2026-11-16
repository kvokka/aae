# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-16 ver=aae-0.1.0@6699f51-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 480 s and 1 rounds from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@6699f51-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 480 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; coder on backend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 929166, output 16599.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ f-dqm [● P2] [task] - Build browser Snake game with high-score leaderboard
✓ api-9a1 [● P2] [task] - Build persistent Snake high-score API
✓ root-p2p [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
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
summary: Backend: started with python3 server.py as documented, using the documented SNAKE_DATABASE override for temporary test data; posted scores, restarted the server, and GET returned both scores. Gameplay: served the frontend with python3 -m http.server 5173, opened it in headless agent-browser, started a game, pressed arrow keys for several seconds, and inspected before and after screenshots showing the snake moved. Scores: played until game over, submitted Browser QA with score 0 in the page, saw success and the leaderboard update, then reloaded after backend restart and saw both persisted scores. Stopped the browser and both servers; screenshots and database are in /tmp/tmp.KKzz50uvlP.
```

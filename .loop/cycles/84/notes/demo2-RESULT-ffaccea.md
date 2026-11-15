# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-15 ver=aae-0.1.0@71a6bb2-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 777 s and 1 rounds from the request, n=1" -->

Run on 2026-11-15 with aae-0.1.0@71a6bb2-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 777 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1690212, output 28068.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ root-e8s [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-e8s.1 [● P2] [task] - Build persistent high-score API
└── ✓ root-e8s.2 [● P2] [task] - Build browser Snake game and leaderboard
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
summary: Backend: ran the README command python3 server.py on an unchanged copy in a temporary directory to keep scores out of the rig; posted QA Check 12 and a browser score, restarted the server, and GET returned both scores. Game: served the frontend with python3 -m http.server 5173, opened it in headless agent-browser, started it, pressed arrow keys over several seconds, and inspected before and after screenshots showing the snake moved. Submission and leaderboard: the snake ate food and finished with score 1; submitted Browser Player through the game, saw Score saved and both scores in the page, then restarted the backend and reloaded the page and saw both scores persist. Stopped the browser and both servers; artifacts are in /tmp/tmp.AmnbIwQi98.
```

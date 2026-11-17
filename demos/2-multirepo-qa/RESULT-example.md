# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-16 ver=loop-manager-0.1.0@b32c55c,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 769 s and 1 rounds from the request, n=1" -->

Run on 2026-11-16 with loop-manager-0.1.0@b32c55c,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 769 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; coder on backend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1377333, output 24837.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-jse: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── f-tdp: Build Snake browser game and leaderboard [P2] [closed]
  ├── api-y9u: Build persistent high-score API [P2] [closed]
    ├── f-tdp: Build Snake browser game and leaderboard [P2] [closed]
```

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.deliver | qa | root | dev.qa | passed |
| run_2 | dev.implement | code | backend | dev.coder | passed |
| run_3 | dev.implement | code | frontend | dev.coder | passed |

The outcome of the run (`loop-manager run wait`):

```
id: run_1
reason: pass
state: passed
summary: Backend: started with npm start and a temporary SCORE_FILE, posted QA persistence score 12, restarted with npm start, and GET returned the same score. Game: opened frontend started with npm start in headless agent-browser; compared full-board screenshots before and after arrow-key play and saw the snake move from the center to the upper-left. Scores: played until game over, submitted QA player score 0 in the page, saw the saved confirmation and both scores in the leaderboard, and confirmed both via GET. Stopped both servers and the browser; artifacts are in /tmp/tmp.zQYTX32Xsm.
```

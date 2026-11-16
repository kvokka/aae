# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-16 ver=aae-0.1.0@11839e9-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 606 s and 1 rounds from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@11839e9-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 606 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; coder on backend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1116169, output 18909.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-2ph: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── f-c8g: Build browser Snake game and high-score integration [P2] [closed]
  ├── api-wha: Build persistent high-score API [P2] [closed]
    ├── f-c8g: Build browser Snake game and high-score integration [P2] [closed]
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
summary: Backend: ran npm install and npm start as documented with SCORE_DATA_FILE in a temporary directory; after a browser score submission, restarted the backend and GET still returned QAPlayer 0. Game: ran the frontend as documented, opened it with headless agent-browser, started a game, pressed arrow keys over several seconds, and inspected before and after screenshots showing the snake moved and changed orientation. Scores: let the game end, submitted QAPlayer with score 0, saw Score saved and the leaderboard entry, then reloaded after backend restart and saw the same entry. Stopped both servers and closed the browser.
```

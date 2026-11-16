# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-16 ver=aae-0.1.0@9d366e3-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 772 s and 1 rounds from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@9d366e3-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 772 s from the request,
after 1 round(s) of the checks (at most 3).
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; coder on backend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1595538, output 27303.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-z3j: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── f-hs7: Build browser Snake game and high-score UI [P2] [closed]
  ├── api-kju: Build persistent high-score API [P2] [closed]
    ├── f-hs7: Build browser Snake game and high-score UI [P2] [closed]
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
summary: Backend: started with python3 server.py using a temporary score file, posted QA Test 4 and Browser QA 0, restarted the server, and GET returned both scores. Browser game: started the frontend with its README command, opened it in headless agent-browser, started play and sent arrow keys; inspected before and after screenshots showing the snake move and turn. Finished game and leaderboard: played to game over, submitted Browser QA 0 through the form, saw the success message and both scores on the page, and saw them again after restarting the backend and reloading. Stopped both servers and the browser; screenshots and test data are in /tmp/tmp.r0vOfm7neq.
```

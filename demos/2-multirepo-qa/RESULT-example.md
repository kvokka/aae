# 2-multirepo-qa: passed

<!-- MEASURE id=demo-2-multirepo-qa date=2026-11-16 ver=aae-0.1.0@01851cb-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 env=Linux-aarch64 src=demos/2-multirepo-qa/run.sh note="passed in 658 s and 1 rounds from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@01851cb-dirty,codex-0.168.0,rumdl-0.3.4,agent-browser-0.39.0 (`Linux aarch64`): passed in 658 s from the request,
after 1 round(s) of the checks (at most 3).
`-dirty`: the run of demo 1 just before had written its `*-example` files into the tree; the code was `01851cb`.
This is the second of two runs in a row; the first, at `6699f51` (the same aae code, an older demo 1), passed
in 480 s after 1 round.
Models: planner on root: codex gpt-6.1-sol; coder on frontend: codex gpt-6-luna; coder on backend: codex gpt-6-luna; qa on root: codex gpt-6.1-terra.
Tokens the harnesses reported for the runs: input 1058212, output 24612.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-2g5: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── f-7rk: Build browser Snake game and high-score UI [P2] [closed]
  ├── api-ifg: Build persistent high-score API [P2] [closed]
    ├── f-7rk: Build browser Snake game and high-score UI [P2] [closed]
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
summary: Backend: started with the README command and a temporary database, submitted Persistence check 42, restarted the backend, and retrieved the same score. Browser gameplay: served the frontend with its README command, opened it in headless agent-browser, took screenshots before and after arrow-key play, and observed the snake move; it then hit a wall and ended the game. Score integration: submitted the finished game as Browser player with score 0, saw the success message and both scores in the page ranking, and confirmed both via the API. Stopped the browser and both servers; screenshots and database are in /tmp/tmp.CWgSjPdnV6.
```

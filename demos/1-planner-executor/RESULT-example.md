# 1-planner-executor: passed

<!-- MEASURE id=demo-1-planner-executor date=2026-11-16 ver=loop-manager-0.1.0@b32c55c,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/1-planner-executor/run.sh note="passed in 286 s from the request, n=1" -->

Run on 2026-11-16 with loop-manager-0.1.0@b32c55c,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 286 s from the request.
Models: planner on root: claude haiku; coder on frontend: claude haiku; coder on backend: codex gpt-6-luna.
Tokens the harnesses reported for the runs: input 177502, output 35183.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-0j3: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── f-034: Snake game: browser UI with leaderboard [P2] [closed]
  ├── api-pl2: Snake high-score backend: JSON API [P2] [closed]
    ├── f-034: Snake game: browser UI with leaderboard [P2] [closed]
```

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.deliver | report | root |  | passed |
| run_2 | dev.implement | code | backend | dev.coder | passed |
| run_3 | dev.implement | code | frontend | dev.coder | passed |

The outcome of the run (`loop-manager run wait`):

```
data.exit_code: 0
data.stderr:
data.stdout: root-0j3: Create me snake game in browser and a simple backend which store the high-scores [P2] [in_progress]
  ├── f-034: Snake game: browser UI with leaderboard [P2] [closed]
  ├── api-pl2: Snake high-score backend: JSON API [P2] [closed]
    ├── f-034: Snake game: browser UI with leaderboard [P2] [closed]
drain: {"closed":["api-pl2","f-034"],"failed":[],"refused":{}}

id: run_1
reason: pass
state: passed
summary: exit 0
```

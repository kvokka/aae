# 1-planner-executor: passed

<!-- MEASURE id=demo-1-planner-executor date=2026-11-16 ver=aae-0.1.0@11839e9-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/1-planner-executor/run.sh note="passed in 280 s from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@11839e9-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 280 s from the request.
Models: planner on root: claude haiku; coder on frontend: claude haiku; coder on backend: codex gpt-6-luna.
Tokens the harnesses reported for the runs: input 157194, output 36187.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-ulf: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── api-mmr: Backend: high-score REST API [P2] [closed]
    ├── f-l1j: Frontend: browser snake game with high-score table [P2] [closed]
  ├── f-l1j: Frontend: browser snake game with high-score table [P2] [closed]
```

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.deliver | report | root |  | passed |
| run_2 | dev.implement | code | backend | dev.coder | passed |
| run_3 | dev.implement | code | frontend | dev.coder | passed |

The outcome of the run (`aae run wait`):

```
data.exit_code: 0
data.stderr:
data.stdout: root-ulf: Create me snake game in browser and a simple backend which store the high-scores [P2] [in_progress]
  ├── api-mmr: Backend: high-score REST API [P2] [closed]
    ├── f-l1j: Frontend: browser snake game with high-score table [P2] [closed]
  ├── f-l1j: Frontend: browser snake game with high-score table [P2] [closed]
drain: {"closed":["api-mmr","f-l1j"],"failed":[],"refused":{}}

id: run_1
reason: pass
state: passed
summary: exit 0
```

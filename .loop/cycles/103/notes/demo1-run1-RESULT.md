# 1-planner-executor: failed

<!-- MEASURE id=demo-1-planner-executor date=2026-11-16 ver=aae-0.1.0@6699f51-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/1-planner-executor/run.sh note="failed in 357 s from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@6699f51-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): failed in 357 s from the request.
Models: planner on root: claude haiku; coder on frontend: claude haiku; coder on backend: codex gpt-6-luna.
Tokens the harnesses reported for the runs: input 195039, output 42030.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ f-acg [● P2] [task] - Frontend: browser Snake game with high-score table
✓ api-huq [● P2] [task] - Backend: high-score store API (Python stdlib)
○ root-qk7 [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
```

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.deliver | report | root |  | failed |
| run_2 | dev.implement | code | backend | dev.coder | passed |
| run_3 | dev.implement | code | frontend | dev.coder | passed |

The outcome of the run (`aae run wait`):

```
class: agent
data.exit_code: 1
data.stderr:
data.stdout: ✓ f-acg [● P2] [task] - Frontend: browser Snake game with high-score table
✓ api-huq [● P2] [task] - Backend: high-score store API (Python stdlib)
◐ root-qk7 [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
drain: {"closed":["api-huq","f-acg"],"failed":[],"refused":{}}

id: run_1
reason: exit 1
state: failed
summary: exit 1
```

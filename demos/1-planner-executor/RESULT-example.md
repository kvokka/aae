# 1-planner-executor: passed

<!-- MEASURE id=demo-1-planner-executor date=2026-11-15 ver=aae-0.1.0@a3a6be0-dirty,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/1-planner-executor/run.sh note="passed in 236 s from the request, n=1" -->

Run on 2026-11-15 with aae-0.1.0@a3a6be0-dirty,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 236 s from the request.
Models: planner on root: claude haiku; coder on frontend: claude haiku; coder on backend: codex gpt-6-luna.
Tokens the harnesses reported for the runs: input 133895, output 29051.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ root-eek [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-eek.1 [● P2] [task] - Snake high-score backend
└── ✓ root-eek.2 [● P2] [task] - Snake browser game frontend
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
data.stdout: ◐ root-eek [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-eek.1 [● P2] [task] - Snake high-score backend
└── ✓ root-eek.2 [● P2] [task] - Snake browser game frontend
drain: {"closed":["root-eek.1","root-eek.2"],"failed":[],"refused":{}}

id: run_1
reason: pass
state: passed
summary: exit 0
```

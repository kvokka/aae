# 1-planner-executor: passed

<!-- MEASURE id=demo-1-planner-executor date=2026-11-15 ver=aae-0.1.0@c1fa9bc,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/1-planner-executor/run.sh note="passed in 347 s from the request, n=1" -->

Run on 2026-11-15 with aae-0.1.0@c1fa9bc,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 347 s from the request.
Models: planner on root: claude haiku; coder on frontend: claude haiku; coder on backend: codex gpt-6-luna.
Tokens the harnesses reported for the runs: input 141773, output 50321.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads (`br list --all --tree`):

```
✓ root-99i [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-99i.1 [● P2] [task] - Snake high-score backend (Python stdlib)
└── ✓ root-99i.2 [● P2] [task] - Browser Snake game with high-score table
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
data.stdout: ◐ root-99i [● P2] [epic] - Create me snake game in browser and a simple backend which store the high-scores
├── ✓ root-99i.1 [● P2] [task] - Snake high-score backend (Python stdlib)
└── ✓ root-99i.2 [● P2] [task] - Browser Snake game with high-score table
drain: {"closed":["root-99i.1","root-99i.2"],"failed":[],"refused":{}}

id: run_1
reason: pass
state: passed
summary: exit 0
```

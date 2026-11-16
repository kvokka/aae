# 1-planner-executor: passed

<!-- MEASURE id=demo-1-planner-executor date=2026-11-16 ver=aae-0.1.0@01851cb,claude-2.1.325,codex-0.168.0 env=Linux-aarch64 src=demos/1-planner-executor/run.sh note="passed in 423 s from the request, n=1" -->

Run on 2026-11-16 with aae-0.1.0@01851cb,claude-2.1.325,codex-0.168.0 (`Linux aarch64`): passed in 423 s from the request.
Models: planner on root: claude haiku; coder on frontend: claude haiku; coder on backend: codex gpt-6-luna.
Tokens the harnesses reported for the runs: input 186598, output 50819.

The request: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

The beads under the request (`br dep tree <root> --direction up`):

```
root-qaw: Create me snake game in browser and a simple backend which store the high-scores [P2] [closed]
  ├── api-z9d: Score API: Python stdlib HTTP server storing top-10 high scores [P2] [closed]
    ├── f-c42: Snake game: static browser frontend using the score API [P2] [closed]
  ├── f-c42: Snake game: static browser frontend using the score API [P2] [closed]
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
data.stdout: root-qaw: Create me snake game in browser and a simple backend which store the high-scores [P2] [in_progress]
  ├── api-z9d: Score API: Python stdlib HTTP server storing top-10 high scores [P2] [closed]
    ├── f-c42: Snake game: static browser frontend using the score API [P2] [closed]
  ├── f-c42: Snake game: static browser frontend using the score API [P2] [closed]
drain: {"closed":["api-z9d","f-c42"],"failed":[],"refused":{}}

id: run_1
reason: pass
state: passed
summary: exit 0
```

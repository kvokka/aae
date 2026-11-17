# 3-review-gate: failed

<!-- MEASURE id=demo-3-review-gate date=2026-11-17 ver=aae-0.1.0@e67257a-dirty,codex-0.168.0,gh-2.103.2 env=Linux-aarch64 src=demos/3-review-gate/run.sh note="failed in 485 s and 1 rounds from the request, n=1" -->

Run on 2026-11-17 with aae-0.1.0@e67257a-dirty,codex-0.168.0,gh-2.103.2 (`Linux aarch64`): failed in 485 s from the request,
after 1 round(s) of the review.
Models: planner on root: codex gpt-6.1-terra; coder on backend: codex gpt-6-luna; coder on frontend: codex gpt-6-luna; reviser on root: codex gpt-6-luna; owner on root: codex gpt-6.1-terra.

The request: "Let a visitor of the site type their name and see a greeting the backend makes for it, KISS". The root's pull request: https://github.com/kvokka-agent/gate-demo/pull/1.

The runs, oldest first:

| run | formula | step | rig | agent | state |
|---|---|---|---|---|---|
| run_1 | dev.feature | review | gate-demo |  | cancelled |
| run_2 | dev.feature-base | work | gate-demo |  | passed |
| run_3 | dev.implement | code | gate-demo-backend | dev.coder | passed |
| run_4 | dev.implement | code | gate-demo-frontend | dev.coder | failed |
| run_5 | review-gate.owner-review | review | gate-demo | review-gate.owner | failed |
| run_6 | dev.cleanup | cleanup | gate-demo |  | running |

The outcome of the run (`aae run wait`):

```
id: run_1
reason: the owner's gh had no login (cycle 118, run 2)
state: cancelled
```

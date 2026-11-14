# Demos

Live examples of aae (spec §15, criterion 20): one general pack, and a user who gives its agents a request. They run
real models, so they need the harness logins and spend tokens; CI only checks that they load.

- [`dev-pack/`](dev-pack): the pack `dev`, usable in any project. Agent `planner` splits a request into independent
  tasks per rig and hands them out; agent `coder` writes code and tests it before it says it works. Formula `plan`
  (input `request`) is the planner's process: find the rigs, write self-contained tasks, start `dev.code` on each
  rig, wait with `aae run wait`, retry a failed part once, finish with a summary. Formula `code` (input `task`) is
  the worker's: do it, test it, leave nothing running, finish with the data its neighbours need. Not a word of any
  particular task.
- A demo is four directories: the pack and, per demo, `root/` (the user's repository: `config.toml` for the daemon,
  `aae.toml` with the rigs and the user's settings, `REQUEST.md` with what the user wants, `smoke.sh` with how the
  user checks it) and two rigs, `backend/` and `frontend/`, empty before a run and the snapshot of the last green run
  after.

| Demo | Planner | Backend worker | Frontend worker |
|---|---|---|---|
| [`snake-claude`](snake-claude) | claude haiku | claude haiku | claude haiku |
| [`snake-mixed`](snake-mixed) | claude haiku | codex luna (variant `codex-coder`) | claude haiku |

The request is a browser snake game whose backend keeps the best scores in a plain-text file. The planner runs in
`root`, starts one `dev.code` run per rig (child runs: same correlation, one level deeper) and waits for them; its
pass fires the project's trigger, which runs the user's `smoke.sh` as the exec formula `smoke` of the project's own
pack.

## Run

```sh
cd demos
mise run snake-claude          # or snake-mixed; AAE=/path/to/aae if aae is not on PATH
mise run check                 # what CI runs: both demos load
```

`run.sh` copies the pack and the demo's `root/` into a temporary directory with two empty rigs, starts a daemon of
its own there (`XDG_*` and `AAE_SOCKET` point into it; `inherit_host = false` keeps the host's harness settings out,
only the logins come from your host) and runs

```sh
aae run start <demo> root dev.plan --input request="$(cat REQUEST.md)"
```

then waits with `aae run wait` for the planner and the smoke run. On a pass it copies the rigs back into
`demos/<demo>/` with `RESULT.md`: the runs and who started each. `cargo run -p aae-acceptance -- 20` runs both demos
as rows of the acceptance report.

## Roadmap: demo 3

aae develops itself: the agent setup of this repository as an aae pack. Not in the first release.

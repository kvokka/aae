# Demos

Small live examples of aae (spec §15, criterion 20): a pack with its agents, formulas and triggers, and what the
agents made with it. They run real models, so they need the harness logins and spend tokens; CI only checks that
they load.

| Demo | Backend agent | Frontend agent |
|---|---|---|
| [`snake-claude`](snake-claude) | claude haiku | claude haiku |
| [`snake-mixed`](snake-mixed) | codex `gpt-5.6-luna` | claude haiku |

Both use the pack [`snake`](snake): two empty rigs, `backend` and `frontend`, and the task of a browser snake game
whose backend keeps the best scores in a plain-text file. The two projects differ only in
`agents/backend/agent.toml`.

The chain, all of it in the core:

1. `aae run start <demo> backend snake.backend`: the backend agent writes `server.py` (Python standard library) and
   finishes with the API in `outcome.data` (port, `GET` and `POST` paths).
2. The trigger `frontend-after-backend` in `aae.toml` listens to `run.passed` on the `backend` rig and starts
   `snake.frontend` on the `frontend` rig with that API as its inputs; the agent writes `index.html` (plain JS).
3. The trigger `smoke-after-frontend` starts `snake.smoke`, an `exec` step: [`smoke.sh`](snake/smoke.sh) starts the
   backend, posts a score, checks `scores.txt` and `GET /scores`, and checks the page has the game. Exit 0 is a pass.

## Run

```sh
cd demos
mise run snake-claude          # or snake-mixed; AAE=/path/to/aae if aae is not on PATH
mise run check                 # what CI runs: the pack and projects load
```

`run.sh` copies the pack and the project into a temporary directory with empty rigs, starts a daemon of its own
there (`XDG_STATE_HOME` and `AAE_SOCKET` point into it; the agents are isolated with `inherit_host = false`, only
the logins come from your host), starts the backend run and waits for the smoke run, 18 minutes at most. Then it
copies the rigs back into `demos/<demo>/` as the snapshot, with the run list in `RESULT.md`.
`cargo run -p aae-acceptance -- 20` runs both demos as rows of the acceptance report.

## Roadmap: demo 3

aae develops itself: the agent setup of this repository as an aae pack. Not in the first release.

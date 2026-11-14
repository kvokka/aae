# Demo 1: a planner and its workers on claude

The pack [`dev`](../dev-pack) with every agent on claude haiku. [`root/aae.toml`](root/aae.toml) is all the user
configures: the rigs `root`, `backend`, `frontend`, and the cheapest model for both agents of the pack as an
override (`[overrides.planner]`, `[overrides.coder]`) over the pack's defaults. The harness is the pack's default,
`claude`; the user does not name it.

- [`root/REQUEST.md`](root/REQUEST.md): what the user wants, in their words.
- [`root/smoke.sh`](root/smoke.sh): how the user checks it; [`root/formulas/smoke.toml`](root/formulas/smoke.toml)
  runs it when the planner passes.
- [`backend/`](backend), [`frontend/`](frontend): the snapshot of the last green run (`server.py`, `index.html`), and
  [`RESULT.md`](RESULT.md) with the runs.

Run it with `mise run snake-claude` in `demos/`.

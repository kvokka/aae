# Demo 2: snake on claude and codex

Demo 1 with mixed harnesses: the backend agent runs on codex with its cheapest model (`agents/backend/agent.toml`
is the only file that differs from [demo 1](../snake-claude)), the frontend agent of the [`snake`](../snake) pack on
claude haiku.

The snapshot of the last green run: [`backend/`](backend), [`frontend/`](frontend) (open `index.html` while
`PORT=8765 python3 backend/server.py` runs) and [`RESULT.md`](RESULT.md) with the runs of the chain. Run it again
with `mise run snake-mixed` in `demos/`.

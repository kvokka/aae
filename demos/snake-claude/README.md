# Demo 1: snake on claude

The project of the [`snake`](../snake) pack with both agents on claude haiku. `agents/backend/agent.toml` is its only
agent; the frontend agent and the formulas come from the pack.

The snapshot of the last green run: [`backend/`](backend) (`server.py` and the `scores.txt` the smoke test wrote),
[`frontend/`](frontend) (`index.html`; open it in a browser while `PORT=8765 python3 backend/server.py` runs) and
[`RESULT.md`](RESULT.md) with the runs of the chain. Run it again with `mise run snake-claude` in `demos/`.

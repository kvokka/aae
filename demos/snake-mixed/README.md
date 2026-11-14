# Demo 2: the backend worker on codex

The same pack and request as [demo 1](../snake-claude); only [`root/aae.toml`](root/aae.toml) differs. It declares a
variant of the pack's coder on another provider and runs it on the rig `backend`:

```toml
[agents.codex-coder]
base = "dev.coder"
provider = "openai"                   # aae picks the harness that serves it: codex
model = "luna"                        # the codex adapter expands it to the latest luna
permission_mode = "never"

[rigs.backend]
path = "../backend"
agents = { coder = "codex-coder" }
```

The other way to the same change, without a new agent name, is an override on the rig:

```toml
[rigs.backend.overrides.coder]
provider = "openai"
model = "luna"
permission_mode = "never"
```

`aae config explain --config root/config.toml --project snake-mixed --formula dev.code --rig backend` shows which
agent runs there and where each value came from. The snapshot of the last green run is in [`backend/`](backend),
[`frontend/`](frontend) and [`RESULT.md`](RESULT.md). Run it with `mise run snake-mixed` in `demos/`.

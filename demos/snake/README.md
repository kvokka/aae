# The snake pack

What the two snake demos share: the frontend agent (claude haiku), the formulas `backend`, `frontend` and `smoke`,
and [`smoke.sh`](smoke.sh). The backend agent is not here: each project brings its own (`agent = "backend"` in
`formulas/backend.toml` resolves to the project's agent), which is how the demos swap the harness of one agent
without touching the pack. The triggers that chain the formulas are in each project's `aae.toml`, since a pack does
not know rig names.

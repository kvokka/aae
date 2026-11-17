# context-budget

A plugin of aae for claude and codex agents: it sets the context window at which the harness compacts a session,
counts the compactions of the session, keeps the label `compactions:<n>` on the run's bead and fails the run past
`max_compactions`.

```toml
# the orchestrator's config.toml or a project's aae.toml
[[plugin]]
path = "<this directory>"            # or git = "<this repository>", rev = "<tag>", dir = "plugins/context-budget"

# any settings layer: [settings], a rig, agent.toml, a variant
plugins = [{ name = "context-budget", params = { window = 150000, max_compactions = 2 } }]
```

## Params

| Param | Default | What |
|---|---|---|
| `window` | `200000` | tokens at which the harness compacts the session: claude's `autoCompactWindow` (in `--settings`), codex's `model_auto_compact_token_limit` (the app-server's config) |
| `max_compactions` | `3` | compactions of a session past which its run fails |

claude takes a window from 100000 to 1000000 and drops a smaller one without a word, compacting at about the window
less 33k tokens; codex takes any size and compacts after the tool call whose reply crosses it.

## How it works

The hook `hooks/budget` runs in the agent's session, on two harness hooks:

- `PostCompact`: the count of the session (by its `session_id`) goes up by one in `<state>/plugins/context-budget/`.
  When the run has a bead (`AAE_BEAD`), the bead's `compactions:*` label is replaced by `compactions:<n>`, written as
  the session's tracker actor. Past `max_compactions` the hook declares the outcome `fail` with the summary
  `context budget: <n> compactions` (`aae finish`) and answers `{"continue": false}`: codex ends the turn there.
- `PreToolUse`: once the session is past its budget, every tool call is denied and the turn stops; this is what
  stops claude, which does not stop on an answer of a compaction hook.

The turn over, the run fails with the declared outcome. A compaction inside a claude subagent or a codex child thread
(its input has `agent_id`) is of that agent's own context and does not count. The count is per session: a new
session of the run, after an attempt failed, starts from zero.

## Needs on the host

`jq`, `br` (the label; it finds the tracker by the session's `BEADS_DIR`, as the agent's own calls do) and `aae` (the
finisher) on the `PATH` of the harness's hooks.

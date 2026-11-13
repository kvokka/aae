# Writing a plugin

A plugin is a directory with a `plugin.toml`. It has two independent sides:

- the **agent side** (`[agent]`): settings the agents that attach the plugin get, such as skills, harness hooks,
  MCP servers and environment variables;
- the **orchestrator side** (`[[hook]]`): programs aae runs on its own events, which may observe them or decide what
  happens next (veto a pass, retry a failed attempt, answer a permission request...).

A plugin is a directory, not a library: hooks are any executables, in any language. The complete example
[`examples/aae-plugin-review`](../../examples/aae-plugin-review) shows both sides; this page walks through it.

## The manifest

```toml
name = "review"
version = "0.1.0"

[agent]
skills.review-checklist = { path = "skills/review-checklist" }
hooks.review-reminder = { event = "UserPromptSubmit", command = "${AAE_PLUGIN_DIR}/hooks/remind.sh", timeout = "10s" }

[[hook]]
on = "run.finishing"
command = ["bin/require-word"]
timeout = "10s"
```

Plugin names are unique across the orchestrator and the project. Unknown keys are an error.

## Declare it, then attach it

A plugin is **declared** in the orchestrator config or in a project's `aae.toml`, with its parameters:

```toml
[[plugin]]
path = "~/aae-plugins/aae-plugin-review"
config = { require = "tested" }
```

Declaring it turns its orchestrator side on. Its agent side reaches only the agents that **attach** it, in their
`agent.toml` or in any `[settings]` layer:

```toml
plugins = [{ name = "review" }]
```

A stronger layer can detach it again with `{ name = "review", enabled = false }`, and with it its whole agent-side
contribution. `aae config explain --project <p> --agent <a>` shows which plugin each value came from, and
`aae plugin list` lists the declared and the bundled plugins.

## The agent side

The `[agent]` table takes the same keys as any settings layer (see [the config reference](../design/config.md)):
`env`, `mcp.<name>`, `hooks.<name>`, `skills.<name>`, `native.<harness>`, `model` and so on. It sits between the
rig settings and the agent's own `agent.toml`, so an agent can still override what the plugin sets.

- **Skills**: `skills.<name> = { path = "..." }`, a directory relative to the plugin with a `SKILL.md`.
- **Harness hooks** use the Claude Code dialect (`event`, `matcher`, `command`, `timeout`); aae delivers them to
  claude as they are and translates them for codex. What `remind.sh` prints on `UserPromptSubmit` joins the prompt.
- `${AAE_PLUGIN_DIR}` in a hook or MCP `command` is replaced with the plugin's directory when the config loads, so
  a command can reach the plugin's own files.

## The orchestrator side

Each `[[hook]]` names one event type (`on`), a `command` (a path with a `/` is relative to the plugin directory)
and an optional `timeout` (default 30 s, at most 300 s). The full contract is in
[contracts.md, section 3](../design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts); in short:

- **stdin**: the event as one JSON line (`{"schema", "seq", "ts", "type", "project", "rig", "run", "data", ...}`),
  then EOF.
- **cwd**: the plugin directory. **Environment**: only `PATH`, `HOME`, `LANG`, `AAE_SOCKET` (the API socket, to
  call back into aae), `AAE_PLUGIN_DIR` and `AAE_PLUGIN_PARAMS` (the `config` table of the declaration as JSON).
- **Result**: exit 0 with empty stdout means no objection; exit 0 with one JSON object on stdout is a decision;
  exit 2 blocks, with stderr as the reason; anything else (another exit code, a timeout, bad JSON) is a hook
  failure, reported as the `plugin.hook_failed` event.

Hooks on observed events (`run.state_changed`, `run.passed`, `bead.ready`, ...) can only succeed or fail. Hooks on
the deciding events may return a decision:

| Event | Decisions |
|---|---|
| `run.preparing` | `proceed` (optionally with `workdir`, `env`, `prompt_append`), `deny` |
| `run.requires_action` | `allow`, `deny`, `answer` (for a question) |
| `run.attempt_failed` | `resume`, `new_session`, `park` (with a gate), `fail` |
| `run.turn_ended_without_outcome` | `continue` (with a message), `fail` |
| `run.finishing` | `accept`, `veto`, `remark` (sends the agent back with a message) |

On `run.preparing` and `run.finishing` a failed hook counts as a veto: a broken check never lets a run through.
On the others a failure is just "no decision", and the next plugin, then the bundled `default-policy`, decides.

`bin/require-word` is a `run.finishing` veto: it exits 2 when a `pass` summary does not contain the word given in
the `require` parameter, so the run fails with class `check` and the reason from stderr.

## Rules worth keeping

- **Be idempotent.** Delivery is at least once: a hook cut by a daemon crash runs again after the restart, with the
  same `seq`.
- **Be quick.** A deciding hook holds its run until it answers or times out.
- **Parse JSON properly.** The example uses `sed` to stay dependency-free; a real hook should use `jq` or a real
  JSON parser.
- Several plugins on one event run in declaration order (orchestrator first, then the project); the bundled
  plugins come last, so a declared plugin always decides first.

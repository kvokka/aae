# Writing a plugin

A plugin is a directory with a `plugin.toml`. It has two independent sides:

- the **agent side** (`[agent]`): settings the agents that attach the plugin get, such as skills, harness hooks,
  MCP servers and environment variables;
- the **orchestrator side** (`[[hook]]`): programs aae runs on its own events, which may observe them or decide what
  happens next (veto a pass, retry a failed attempt, remark on a turn...).

A plugin is not a pack: a pack brings agents and formulas, a plugin extends the agents that attach it and the
orchestrator. A plugin is a directory, not a library: hooks are any executables, in any language. The full reference
is [the config reference](../design/config.md#plugin-pluginplugintoml).

## The manifest

```toml
name = "review"
version = "0.1.0"
harnesses = ["claude", "codex"]   # optional: the harnesses it is for

[params]                          # optional: defaults, which give the types (string, integer, boolean)
require = "tested"

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

A plugin is **declared** in the orchestrator config or in a project's `aae.toml`; the declaration only says where it
is:

```toml
[[plugin]]
path = "~/aae-plugins/review"
```

A plugin in a git repository is declared with `git = "<url>"` and `rev = "<tag or full commit id>"` instead of `path`,
and `dir = "<directory>"` when it is not at the root of the repository; `rev` is required for a plugin. `aae` downloads
it into its cache when it loads the config and runs it only at that tag or commit; see
[Packs and plugins from git](../design/config.md#packs-and-plugins-from-git).

Declaring it turns nothing on: both sides work only where the plugin is **attached**. Its agent side reaches the
agents that attach it, and its orchestrator hooks run on the events of their runs; the events of no agent (`bead.*`,
`service.*`, exec steps) go to the plugins attached in `[settings]` of the orchestrator or the project. Attach it in
`[settings]` of the orchestrator or a project for every agent there, on a rig, or in an `agent.toml`, a variant or an
edit for one agent. An attachment may give values to the plugin's parameters:

```toml
plugins = [{ name = "review", params = { require = "reviewed" } }]
```

Each parameter takes its default, then the value each layer that attaches the plugin gives, the strongest last. A
stronger layer can detach the plugin with `{ name = "review", enabled = false }`, and with it its whole agent-side
contribution. `aae config explain --project <p> --agent <a>` shows which plugin each value came from, and
`aae plugin list` lists the declared and the bundled plugins; the explanation shows each parameter's value and its
layer as `plugins.review.params.require`.

## The agent side

The `[agent]` table takes five keys of a settings layer (see [the config reference](../design/config.md)): `env`,
`mcp.<name>`, `hooks.<name>`, `skills.<name>` and `native.<harness>`. It sits between the rig settings and the
agent's own `agent.toml`, so an agent can still override what the plugin sets.

- **Skills**: `skills.<name> = { path = "..." }`, a directory relative to the plugin with a `SKILL.md`.
- An aae plugin is not handed to claude as a Claude Code plugin directory (`--plugin-dir`): its skills, harness hooks
  and MCP servers reach the session only through `[agent]`.
- **Harness hooks** use the Claude Code dialect (`event`, `matcher`, `command`, `timeout`); claude and codex take
  them in the same form, and for codex aae puts the attempt environment in front of the command. What `remind.sh`
  prints on `UserPromptSubmit` joins the prompt.
- `${params.<key>}` in a string of `[agent]` or a `[[service]]` is the parameter's value for the agent at hand; a
  `native` string that is only that reference takes the value with its type, so `window = "${params.window}"` is an
  integer.
- `${AAE_PLUGIN_DIR}` in a hook, MCP or service `command`, in an `env` value and in a service's `health.command`, is
  replaced with the plugin's directory, so a command can reach the plugin's own files; in an `env` the directory is
  literal, even with a `$` in its path. `${AAE_PLUGIN_DATA}` is a directory the daemon makes for the plugin,
  `<state>/plugins/<name>/`, for the files its hooks keep. Any other `${NAME}` in the `[agent]` `env` is a
  variable of the daemon's environment, resolved when the session starts; an MCP or service `env` takes none
  ([environment values](../design/config.md#environment-values)).
- **Scripts**: the plugin's `bin/` directory comes first on the `PATH` of the attached agents' sessions.
- **A prompt fragment**: the top-level key `prompt = "prompt.md"` names a file whose text is appended to the
  attached agents' system prompt.
- **A service**: a `[[service]]` table runs one process for all sessions instead of one per session, for example
  `[[service]]` with `name = "notes"`, `command = ["${AAE_PLUGIN_DIR}/bin/notes-mcp"]` and `mcp = true`. The process
  listens on `$AAE_SERVICE_PORT` and checks `Authorization: Bearer $AAE_SERVICE_TOKEN`; the sessions get its MCP
  endpoint `/mcp` (or, without `mcp`, its URL and token in `AAE_SERVICE_<NAME>_URL` and `_TOKEN`)
  ([details](../design/config.md#mcp-servers-and-services)).

## The orchestrator side

Each `[[hook]]` names one event type (`on`), a `command` (a path with a `/` is relative to the plugin directory)
and an optional `timeout` (default 30 s, at most 300 s). The full contract is in
[contracts.md, section 3](../design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts); in short:

- **stdin**: the event as one JSON line (`{"schema", "seq", "ts", "type", "project", "rig", "run", "data", ...}`),
  then EOF.
- **cwd**: the plugin directory. **Environment**: only `PATH`, `HOME`, `LANG`, `AAE_SOCKET` (the API socket, to
  call back into aae), `AAE_PLUGIN_DIR`, `AAE_PLUGIN_PARAMS` (the params in force for the run's agent as JSON) and
  `AAE_INSTANCE` (a marker: after a daemon crash the next daemon kills every process that still carries it).
- **Result**: exit 0 with empty stdout means no objection; exit 0 with one JSON object on stdout is a decision;
  exit 2 blocks, with stderr as the reason; anything else (another exit code, a timeout, bad JSON) is a hook
  failure, reported as the `plugin.hook_failed` event.

Hooks on observed events (`run.state_changed`, `run.passed`, `bead.ready`, ...) can only succeed or fail. Hooks on
the deciding events may return a decision:

| Event | Decisions |
|---|---|
| `run.preparing` | `proceed` (optionally with `workdir`, `env`, `prompt_append`), `deny` |
| `run.attempt_failed` | `resume`, `new_session`, `park` (with a gate), `fail` |
| `run.turn_ended_without_outcome` | `continue` (with a message), `fail` |
| `run.finishing` | `accept`, `veto`, `remark` (sends the agent back with a message) |

On `run.preparing` and `run.finishing` a failed hook counts as a veto: a broken check never lets a run through.
On the others a failure is just "no decision", and the next plugin, then the bundled `default-policy`, decides.

## Rules worth keeping

- **Be idempotent.** Delivery is at least once: a hook cut by a daemon crash runs again after the restart, with the
  same `seq`, if its run is still waiting for the decision on that event — on `run.preparing`, `run.finishing`,
  `run.attempt_failed` and `run.turn_ended_without_outcome` it is, and its deadline counts again from the restart.
  The copy the dead daemon left running is killed first. After a restart, a `remark` or `continue` cannot reach the
  session that took the turn; the attempt fails as `daemon_crash` and the policy decides.
- **Be quick.** A deciding hook holds its run until it answers or times out.
- **Parse JSON properly**, with `jq` or a real JSON parser.
- Several plugins on one event run in declaration order (orchestrator first, then the project); the bundled
  plugins come last, so a declared plugin always decides first.

# Writing a plugin

A plugin is a directory with a `plugin.toml`. It has two independent sides:

- the **agent side**: what the sessions of the agents that attach it get (skills, harness hooks, MCP servers,
  services, environment variables, native harness keys, a prompt fragment, scripts on `PATH`);
- the **orchestrator side** (`[[hook]]`): programs aae runs on the events of those agents' runs, which may observe
  them or decide what happens next (veto a pass, retry a failed attempt, remark on a turn...).

A plugin is not a pack: a pack brings agents and formulas, a plugin extends the agents that attach it. It is a
directory, not a library: hooks are any executables, in any language. How a plugin is declared and attached is in
[using plugins](plugins.md); every rule of the manifest is in
[the config reference](../design/config.md#plugin-pluginplugintoml).

## The manifest

A plugin `review` that reminds the agent of a word its summary must hold, and sends a pass back without it:

```text
review/
  plugin.toml
  prompt.md                    # joins the system prompt of the attached agents
  skills/review-checklist/     # a skill: SKILL.md and its files
  hooks/remind                 # a harness hook, run in the session
  hooks/check                  # an orchestrator hook, run by the daemon
```

```toml
# ~/aae-plugins/review/plugin.toml
name = "review"
version = "0.1.0"
harnesses = ["claude", "codex"]     # optional: the harnesses it is for; any when absent
prompt = "prompt.md"                # optional

[params]                            # optional: each parameter with its default, which gives its type
word = "tested"                     # string; integer and boolean are the other types

[agent]                             # only env, mcp, hooks, skills and native
env.REVIEW_WORD = "${params.word}"
skills.review-checklist = { path = "skills/review-checklist" }

[agent.hooks.review-remind]
event = "UserPromptSubmit"
command = "'${AAE_PLUGIN_DIR}/hooks/remind' '${params.word}'"
timeout = "10s"

[[hook]]
on = "run.finishing"
command = ["hooks/check"]
timeout = "10s"
```

Unknown keys are an error. Plugin names are unique across the orchestrator and the project.

### Values filled in

The strings of `[agent]` and of a `[[service]]` are filled in for each agent that attaches the plugin, when its
effective config is put together:

- `${params.<key>}` is the parameter's value in force for that agent. A `native` string that is only such a reference
  takes the value with its type: `"${params.window}"` is an integer. A reference to an undeclared key is a load error.
- `${AAE_PLUGIN_DIR}` is the plugin's directory, so a command reaches the plugin's own files; skill paths and
  `prompt` are relative to it.
- `${AAE_PLUGIN_DATA}` is `<state>/plugins/<name>/`, a directory the daemon makes for each declared plugin and never
  reads: the place for the files the plugin's hooks keep.

What is filled in is literal: a `$` in a value put in is no reference. Any other `${NAME}` in an `[agent]` `env`
value is a variable of the daemon's environment, resolved when the session starts
([environment values](../design/config.md#environment-values)).

## The agent side

`[agent]` takes five keys of a settings layer: `env`, `mcp.<name>`, `hooks.<name>`, `skills.<name>` and
`native.<harness>` ([settings](../design/config.md#settings-any-layer)). Besides it the attached agents get:

- the text of the `prompt` file, after the system prompt the layers give;
- the plugin's `bin/` directory, when it has one, ahead of `PATH`;
- its `[[service]]` processes: one process for all sessions instead of one per session.

Harness hooks use the Claude Code dialect (`event`, `matcher`, `command`, `timeout`); claude and codex take them in
the same form, with differences listed in [settings](../design/config.md#settings-any-layer). A hook runs in the
session's environment, which has `AAE_RUN`, `AAE_ATTEMPT_TOKEN`, `AAE_SOCKET` and, when the run has a bead,
`AAE_BEAD`, so it can call `aae` (`aae finish`, for one) and `br` on the run's behalf. What `hooks/remind` prints on
`UserPromptSubmit` joins the prompt:

```sh
#!/bin/sh
# ~/aae-plugins/review/hooks/remind
echo "Before you finish, say in your summary whether the change is $1."
```

On claude a hook fires inside the session's subagents too, with `agent_id` in its input.

A service listens on `$AAE_SERVICE_PORT`, refuses requests without `Authorization: Bearer $AAE_SERVICE_TOKEN`, and
with `mcp` the sessions get its endpoint `/mcp` as an MCP server of the service's name; without it they get
`AAE_SERVICE_<NAME>_URL` and `_TOKEN` ([MCP servers and services](../design/config.md#mcp-servers-and-services)):

```toml
[[service]]
name = "notes"
command = ["${AAE_PLUGIN_DIR}/bin/notes-mcp", "--dir", "${AAE_PLUGIN_DATA}"]
mcp = true
```

An aae plugin is not handed to claude as a Claude Code plugin directory (`--plugin-dir`): its skills, hooks and MCP
servers reach a session only through `[agent]`.

## The orchestrator side

Each `[[hook]]` names one event type (`on`), a `command` (a path with a `/` is relative to the plugin directory) and
an optional `timeout` (default 30 s, at most 300 s). It runs on the events of the runs whose agent attaches the
plugin; the events of no agent (`bead.*`, `service.*`, exec steps) go to the plugins attached in `[settings]` of the
orchestrator or the project. The full contract is in
[contracts.md, section 3](../design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts); in short:

- **stdin**: the event as one JSON line (`{"schema", "seq", "ts", "type", "project", "rig", "run", "data", ...}`),
  then EOF.
- **cwd**: the plugin directory. **Environment**: only `PATH`, `HOME`, `LANG`, `AAE_SOCKET` (to call back into aae),
  `AAE_PLUGIN_DIR`, `AAE_PLUGIN_PARAMS` (the params in force for the run's agent as a JSON object) and `AAE_INSTANCE`.
- **Result**: exit 0 with empty stdout means no objection; exit 0 with one JSON object on stdout is a decision;
  exit 2 blocks, with stderr as the reason; anything else (another exit code, a timeout, bad JSON) is a hook
  failure, reported as the `plugin.hook_failed` event.

`hooks/check` of the example:

```sh
#!/bin/sh
# ~/aae-plugins/review/hooks/check
set -eu
word=$(printf '%s' "$AAE_PLUGIN_PARAMS" | jq -r .word)
summary=$(jq -r 'select(.data.outcome.result == "pass") | .data.outcome.summary')
case $summary in
"" | *"$word"*) ;;
*) jq -nc --arg w "$word" '{decision: "remark", message: "Say whether the change is \($w)."}' ;;
esac
```

Hooks on observed events (`run.state_changed`, `run.passed`, `bead.ready`, ...) can only succeed or fail. Hooks on
the deciding events may return a decision:

| Event | Decisions |
|---|---|
| `run.preparing` | `proceed` (optionally with `workdir`, `env`, `prompt_append`), `deny` |
| `run.attempt_failed` | `resume`, `new_session`, `park` (with a gate), `fail` |
| `run.turn_ended_without_outcome` | `continue` (with a message), `fail` |
| `run.finishing` | `accept`, `veto`, `remark` (sends the agent back with a message) |

On `run.preparing` and `run.finishing` a failed hook counts as a veto: a broken check never lets a run through. On the
others a failure is just "no decision", and the next plugin, then the bundled `default-policy`, decides. Several
plugins on one event run in declaration order (orchestrator first, then the project); the bundled plugins come last.

Rules worth keeping:

- **Be idempotent.** Delivery is at least once: a hook cut by a daemon crash runs again after the restart, with the
  same `seq`, while its run still waits for the decision on that event. After a restart, a `remark` or `continue`
  cannot reach the session that took the turn; the attempt fails as `daemon_crash` and the policy decides.
- **Be quick.** A deciding hook holds its run until it answers or times out.
- **Keep no state in the hook.** An event carries the attempts, times and gates a policy needs; files a plugin must
  keep go in `${AAE_PLUGIN_DATA}`.
- **Parse JSON properly**, with `jq` or a real JSON parser.

## A worked example: `context-budget`

[`plugins/context-budget`](../../plugins/context-budget/README.md) bounds the compactions of a session: it sets the
context window at which the harness compacts, counts the compactions, keeps the label `compactions:<n>` on the run's
bead and fails the run past `max_compactions`. It is all agent side:

```toml
# plugins/context-budget/plugin.toml
name = "context-budget"
version = "0.1.0"
harnesses = ["claude", "codex"]

[params]
window = 200000         # tokens at which the harness compacts the session
max_compactions = 3     # compactions of a session past which its run fails

[agent]
native.claude.autoCompactWindow = "${params.window}"
native.codex.model_auto_compact_token_limit = "${params.window}"

[agent.hooks.context-budget-count]
event = "PostCompact"
command = "'${AAE_PLUGIN_DIR}/hooks/budget' compacted '${AAE_PLUGIN_DATA}' ${params.max_compactions}"

[agent.hooks.context-budget-stop]
event = "PreToolUse"
command = "'${AAE_PLUGIN_DIR}/hooks/budget' tool '${AAE_PLUGIN_DATA}'"
```

- **One parameter, two harnesses.** `window` reaches each harness in its own key: claude's `autoCompactWindow` (in
  its `--settings`) and codex's `model_auto_compact_token_limit`. Each string is only `${params.window}`, so each
  harness gets an integer.
- **State in the data directory.** `hooks/budget compacted` counts by the hook input's `session_id` in
  `${AAE_PLUGIN_DATA}/<session>.count`; the paths are quoted, since the directories may hold spaces.
- **The bead.** With `AAE_BEAD` set, it replaces the bead's `compactions:*` label with `br`, as the session's
  tracker actor.
- **Stopping.** Past the budget it declares the outcome with `aae finish fail --summary ...` and answers
  `{"continue": false}`, which ends a codex turn. claude does not stop on a compaction hook's answer, so the
  `PreToolUse` hook denies every tool call of a session past its budget, and the turn ends. An outcome that is not
  accepted fails the hook and the session goes on: the next compaction tries again.
- **Subagents.** A compaction whose input has `agent_id` is of a subagent's own context and is not counted.

## Testing a plugin

- **The manifest**: a small config that declares and attaches the plugin, then `aae config check --config <file>`
  and `aae config explain --config <file> --project <p> --agent <a>`, run without a daemon. `explain` shows each
  string filled in for that agent and each parameter with its layer.
- **The hooks**: run each one by hand with its input on stdin: a harness hook takes the hook input JSON of the
  harness and the session environment (`AAE_BEAD` and the rest); an orchestrator hook takes an event line and
  `AAE_PLUGIN_PARAMS`. Put stand-ins for the commands it calls (`aae`, `br`) first on its `PATH`.

The tests of `context-budget` do both: `crates/aae/tests/e2e/context_budget.rs` checks with `config explain` that
the params reach each harness and drives `hooks/budget` with the `PostCompact` and `PreToolUse` inputs of claude and
codex, a stand-in `aae` and a real `br` tracker.

## Moving it to its own repository

A plugin needs nothing of the repository it lives in: copy its directory into a repository of its own (at the root,
or in any directory of it), tag a release and declare it with `git` and `rev`, plus `dir` when it is not at the
root:

```toml
[[plugin]]
git = "https://github.com/acme/aae-plugin-review"
rev = "v0.1.0"
```

`rev` is a tag or a full commit id, and the plugin runs only at it; a new version is a new tag and a new `rev`
([packs and plugins from git](../design/config.md#packs-and-plugins-from-git)). Keep its tests in that repository
too.

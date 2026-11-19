# Writing an addon

An addon is a directory with an `addon.toml`. It has two independent sides:

- the **agent side**: what the sessions of the agents that attach it get (skills, harness hooks, MCP servers,
  services, environment variables, native harness keys, a prompt fragment, scripts on `PATH`);
- the **orchestrator side** (`[[hook]]`): programs loop-manager runs on the events of those agents' runs, which may
  observe them or decide what happens next (veto a pass, retry a failed attempt, remark on a turn...).

An addon is not a pack: a pack brings agents and formulas, an addon extends the agents that attach it. It is a
directory, not a library: hooks are any executables, in any language. How an addon is declared and attached is in
[using addons](addons.md); every rule of the manifest is in
[the config reference](../design/config.md#addon-addonaddontoml).

## The manifest

An addon `review` that reminds the agent of a word its summary must hold, and sends a pass back without it:

```text
review/
  addon.toml
  prompt.md                    # joins the system prompt of the attached agents
  skills/review-checklist/     # a skill: SKILL.md and its files
  hooks/remind                 # a harness hook, run in the session
  hooks/check                  # an orchestrator hook, run by the daemon
```

```toml
# ~/loop-manager-addons/review/addon.toml
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
command = "'${LOOP_MANAGER_ADDON_DIR}/hooks/remind' '${params.word}'"
timeout = "10s"

[[hook]]
on = "run.finishing"
command = ["hooks/check"]
timeout = "10s"
```

Unknown keys are an error. Addon names are unique across the orchestrator and the project.

### Values filled in

The strings of `[agent]` and of a `[[service]]` are filled in for each agent that attaches the addon, when its
effective config is put together:

- `${params.<key>}` is the parameter's value in force for that agent. A `native` string that is only such a reference
  takes the value with its type: `"${params.window}"` is an integer. A reference to an undeclared key is a load error.
- `${LOOP_MANAGER_ADDON_DIR}` is the addon's directory, so a command reaches the addon's own files; skill paths and
  `prompt` are relative to it.
- `${LOOP_MANAGER_ADDON_DATA}` is `<state>/addons/<name>/`, a directory the daemon makes for each declared addon and
  never reads: the place for the files the addon's hooks keep.

What is filled in is literal: a `$` in a value put in is no reference. Any other `${NAME}` in an `[agent]` `env`
value is a variable of the daemon's environment, resolved when the session starts
([environment values](../design/config.md#environment-values)).

## The agent side

`[agent]` takes five keys of a settings layer: `env`, `mcp.<name>`, `hooks.<name>`, `skills.<name>` and
`native.<harness>` ([settings](../design/config.md#settings-any-layer)). Besides it the attached agents get:

- the text of the `prompt` file, after the system prompt the layers give;
- the addon's `bin/` directory, when it has one, ahead of `PATH`;
- its `[[service]]` processes: one process for all sessions instead of one per session.

Harness hooks use the Claude Code dialect (`event`, `matcher`, `command`, `timeout`); claude and codex take them in
the same form, with differences listed in [settings](../design/config.md#settings-any-layer). A hook runs in the
session's environment, which has `LOOP_MANAGER_RUN`, `LOOP_MANAGER_ATTEMPT_TOKEN`, `LOOP_MANAGER_SOCKET` and, when the
run has a bead, `LOOP_MANAGER_BEAD`, so it can call `loop-manager` (`loop-manager finish`, for one) and `br` on the
run's behalf. What `hooks/remind` prints on `UserPromptSubmit` joins the prompt:

```sh
#!/bin/sh
# ~/loop-manager-addons/review/hooks/remind
echo "Before you finish, say in your summary whether the change is $1."
```

On claude a hook fires inside the session's subagents too, with `agent_id` in its input.

A service listens on `$LOOP_MANAGER_SERVICE_PORT`, refuses requests without
`Authorization: Bearer $LOOP_MANAGER_SERVICE_TOKEN`, and with `mcp` the sessions get its endpoint `/mcp` as an MCP
server of the service's name; without it they get `LOOP_MANAGER_SERVICE_<NAME>_URL` and `_TOKEN` ([MCP servers and
services](../design/config.md#mcp-servers-and-services)):

```toml
[[service]]
name = "notes"
command = ["${LOOP_MANAGER_ADDON_DIR}/bin/notes-mcp", "--dir", "${LOOP_MANAGER_ADDON_DATA}"]
mcp = true
```

A loop-manager addon is not handed to claude as a Claude Code plugin directory (`--plugin-dir`): its skills, hooks and
MCP servers reach a session only through `[agent]`.

## The orchestrator side

Each `[[hook]]` names one event type (`on`), a `command` (a path with a `/` is relative to the addon directory) and
an optional `timeout` (default 30 s, at most 300 s). It runs on the events of the runs whose agent attaches the
addon; the events of no agent (`bead.*`, `service.*`, exec steps) go to the addons attached in `[settings]` of the
orchestrator or the project. The full contract is in
[contracts.md, section 3](../design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts); in short:

- **stdin**: the event as one JSON line (`{"schema", "seq", "ts", "type", "project", "rig", "run", "data", ...}`),
  then EOF.
- **cwd**: the addon directory. **Environment**: only `PATH`, `HOME`, `LANG`, `LOOP_MANAGER_SOCKET` (to call back into
  loop-manager), `LOOP_MANAGER_ADDON_DIR`, `LOOP_MANAGER_ADDON_PARAMS` (the params in force for the run's agent as a
  JSON object) and `LOOP_MANAGER_INSTANCE`.
- **Result**: exit 0 with empty stdout means no objection; exit 0 with one JSON object on stdout is a decision;
  exit 2 blocks, with stderr as the reason; anything else (another exit code, a timeout, bad JSON) is a hook
  failure, reported as the `addon.hook_failed` event.

`hooks/check` of the example:

```sh
#!/bin/sh
# ~/loop-manager-addons/review/hooks/check
set -eu
word=$(printf '%s' "$LOOP_MANAGER_ADDON_PARAMS" | jq -r .word)
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
others a failure is just "no decision", and the next addon, then the bundled `default-policy`, decides. Several
addons on one event run in declaration order (orchestrator first, then the project); the bundled addons come last.

Rules worth keeping:

- **Be idempotent.** Delivery is at least once: a hook cut by a daemon crash runs again after the restart, with the
  same `seq`, while its run still waits for the decision on that event. After a restart, a `remark` or `continue`
  cannot reach the session that took the turn; the attempt fails as `daemon_crash` and the policy decides.
- **Be quick.** A deciding hook holds its run until it answers or times out.
- **Keep no state in the hook.** An event carries the attempts, times and gates a policy needs; files an addon must
  keep go in `${LOOP_MANAGER_ADDON_DATA}`.
- **Parse JSON properly**, with `jq` or a real JSON parser.

## A worked example: `context-budget`

[`addons/context-budget`](../../addons/context-budget/README.md) bounds the context of a claude session: past `soft`
it tells the agent to wrap up, past `hard` it leaves the state of the work on the run's bead and stops the session,
and it keeps claude from compacting. It is all agent side:

```toml
# addons/context-budget/addon.toml
name = "context-budget"
version = "0.2.0"
harnesses = ["claude"]

[params]
soft = 300000           # tokens of context past which the agent is told to wrap up
hard = 420000           # tokens of context past which the session is stopped
warn_step = 30000       # tokens of growth past `soft` between two notes
cut = "fail"            # the outcome of a run whose session is stopped: "fail" or "pass"

[agent]
# claude compacts at about this window less 33k tokens; the most it takes keeps it past `hard`.
native.claude.autoCompactWindow = 1000000

[agent.hooks.context-budget-start]
event = "SessionStart"
command = "'${LOOP_MANAGER_ADDON_DIR}/hooks/budget' '${LOOP_MANAGER_ADDON_DATA}' ${params.soft} ${params.hard} ${params.warn_step} '${params.cut}'"

[agent.hooks.context-budget-subagent]
event = "SubagentStart"
command = "'${LOOP_MANAGER_ADDON_DIR}/hooks/budget' '${LOOP_MANAGER_ADDON_DATA}' ${params.soft} ${params.hard} ${params.warn_step} '${params.cut}'"

[agent.hooks.context-budget-tool]
event = "PreToolUse"
command = "'${LOOP_MANAGER_ADDON_DIR}/hooks/budget' '${LOOP_MANAGER_ADDON_DATA}' ${params.soft} ${params.hard} ${params.warn_step} '${params.cut}'"

[agent.hooks.context-budget-note]
event = "PostToolUse"
command = "'${LOOP_MANAGER_ADDON_DIR}/hooks/budget' '${LOOP_MANAGER_ADDON_DATA}' ${params.soft} ${params.hard} ${params.warn_step} '${params.cut}'"

[agent.hooks.context-budget-compact]
event = "PreCompact"
command = "'${LOOP_MANAGER_ADDON_DIR}/hooks/budget' '${LOOP_MANAGER_ADDON_DATA}' ${params.soft} ${params.hard} ${params.warn_step} '${params.cut}'"
```

- **One hook, many events.** The same command runs on `SessionStart`, `SubagentStart`, `PreToolUse`, `PostToolUse`
  and `PreCompact`, and reads the event from the input's `hook_event_name`. The params reach it as arguments; the
  paths are quoted, since the directories may hold spaces.
- **The measure.** The context is the usage of the model's last answer in the session's transcript
  (`transcript_path`); a hook input with `agent_id` is a subagent's, measured on its own transcript.
- **Talking to the agent.** The note past `soft` is the `additionalContext` of the `PostToolUse` answer; a call of a
  subagent past `soft` gets a `PreToolUse` deny with the reason.
- **State in the data directory.** The context of the last note, the dump and the stop are files of
  `${LOOP_MANAGER_ADDON_DATA}`, by the input's `session_id`.
- **The bead.** With `LOOP_MANAGER_BEAD` set, the dump past `hard` is a comment on it, written with `br` as the
  session's tracker actor.
- **Stopping.** Past `hard` it declares the outcome with `loop-manager finish <cut>` and answers `{"continue":
  false}`; the `PreToolUse` hook denies every later tool call of the session, and the turn ends.
- **No compaction.** `PreCompact` answers `{"decision": "block"}`, and `autoCompactWindow` is the most claude takes.

## Testing an addon

- **The manifest**: a small config that declares and attaches the addon, then
  `loop-manager config check --config <file>` and
  `loop-manager config explain --config <file> --project <p> --agent <a>`, run without a daemon. `explain` shows each
  string filled in for that agent and each parameter with its layer.
- **The hooks**: run each one by hand with its input on stdin: a harness hook takes the hook input JSON of the
  harness and the session environment (`LOOP_MANAGER_BEAD` and the rest); an orchestrator hook takes an event line and
  `LOOP_MANAGER_ADDON_PARAMS`. Put stand-ins for the commands it calls (`loop-manager`, `br`) first on its `PATH`.

The tests of `context-budget` do both: `crates/loop-manager/tests/e2e/context_budget.rs` checks with `config explain`
that the params reach claude's settings and hooks and drives `hooks/budget` with claude's hook inputs, fake
transcripts, a stand-in `loop-manager` and a real `br` tracker.

## Moving it to its own repository

An addon needs nothing of the repository it lives in: copy its directory into a repository of its own (at the root,
or in any directory of it), tag a release and declare it with `git` and `rev`, plus `dir` when it is not at the
root:

```toml
[[addon]]
git = "https://github.com/acme/loop-manager-addon-review"
rev = "v0.1.0"
```

`rev` is a tag or a full commit id, and the addon runs only at it; a new version is a new tag and a new `rev`
([packs and addons from git](../design/config.md#packs-and-addons-from-git)). Keep its tests in that repository
too.

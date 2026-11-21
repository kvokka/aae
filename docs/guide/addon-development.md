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

[`addons/context-budget`](../../addons/context-budget/README.md) bounds the context of a claude or codex session: past
`soft` it tells the agent to wrap up, past `hard` it leaves the state of the work on the run's bead and stops the
session, and it keeps the harness from compacting. It is all agent side:

```toml
# addons/context-budget/addon.toml
name = "context-budget"
version = "0.3.0"
harnesses = ["claude", "codex"]

[params]
soft = 300000           # tokens of context past which the agent is told to wrap up
hard = 420000           # tokens of context past which the session is stopped
warn_step = 30000       # tokens of growth past `soft` between two notes
cut = "fail"            # the outcome of a run whose session is stopped: "fail" or "pass"

[agent]
# claude compacts at about this window less 33k tokens; the most it takes keeps it past `hard`.
native.claude.autoCompactWindow = 1000000
# codex compacts at `hard`; its `PreCompact` hook stops the session there instead (README.md, "On codex").
native.codex.model_auto_compact_token_limit = "${params.hard}"

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
  (`transcript_path`): claude's last `assistant` entry with `usage`, codex's last `token_count` event. A hook input
  with `agent_id` is a subagent's, measured on its own transcript. A codex input carries `turn_id`, which tells the
  hook which harness it runs for.
- **Talking to the agent.** The note past `soft` is the `additionalContext` of the `PostToolUse` answer; a call of a
  subagent past `soft` gets a `PreToolUse` deny with the reason.
- **State in the data directory.** The context of the last note, the dump and the stop are files of
  `${LOOP_MANAGER_ADDON_DATA}`, by the input's `session_id`.
- **The bead.** With `LOOP_MANAGER_BEAD` set, the dump past `hard` is a comment on it, written with `br` as the
  session's tracker actor.
- **Stopping.** Past `hard` it declares the outcome with `loop-manager finish <cut>` and answers `{"continue":
  false}`; the `PreToolUse` hook denies every later tool call of the session. On claude the turn ends there. A codex
  tool hook cannot end a turn, so codex compacts at `hard` (`model_auto_compact_token_limit`) and the stop is the
  answer to its `PreCompact`.
- **No compaction.** On claude `PreCompact` answers `{"decision": "block"}`, and `autoCompactWindow` is the most claude
  takes; on codex the `{"continue": false}` of `PreCompact` keeps the context and ends the turn.

## Testing an addon

- **The manifest**: a small config that declares and attaches the addon, then
  `loop-manager config check --config <file>` and
  `loop-manager config explain --config <file> --project <p> --agent <a>`, run without a daemon. `explain` shows each
  string filled in for that agent and each parameter with its layer.
- **The hooks**: run each one by hand with its input on stdin: a harness hook takes the hook input JSON of the
  harness and the session environment (`LOOP_MANAGER_BEAD` and the rest); an orchestrator hook takes an event line and
  `LOOP_MANAGER_ADDON_PARAMS`. Put stand-ins for the commands it calls (`loop-manager`, `br`) first on its `PATH`.

The tests of `context-budget` do both: `crates/loop-manager/tests/e2e/context_budget.rs` checks with `config explain`
that the params reach the settings and hooks of claude and codex, and drives `hooks/budget` with the hook inputs of
both harnesses, fake transcripts and rollouts, a stand-in `loop-manager` and a real `br` tracker
(`context_budget/codex.rs` for codex).

## Moving it to its own repository

An addon needs nothing of the repository it lives in: copy its directory into a repository of its own (at the root,
or in any directory of it), tag a release and declare it with `git` and `rev`, plus `dir` when it is not at the
root:

```toml
[[addon]]
git = "https://github.com/acme/loop-manager-addon-review"
rev = "addons/review/0.1.0"
```

`rev` is a tag or a full commit id, and the addon runs only at it; a new version is a new tag and a new `rev`
([packs and addons from git](../design/config.md#packs-and-addons-from-git)). Keep its tests in that repository
too.

## Versioning and releasing an addon

Each addon has its own SemVer in `addon.toml`, a `CHANGELOG.md` beside it and Git tags named
`addons/<name>/<version>`, for example `addons/context-budget/0.3.0`. The numeric tag at the
repository root is reserved for the loop-manager binary. An addon release needs only its source commit
and tag. The same convention works in a separate repository, including one with its manifest at the root.

1. Develop against a relative `path` in the config that declares the addon. For example,
   `devloop/loop-manager.toml` uses `../addons/<name>`. Keep repository-local consumers on these paths.
2. For each user-visible change, add a note under `## Unreleased` in the addon's changelog.
   Describe the effect on users and how to migrate changed parameters or requirements. When preparing a release,
   review the whole section and the diff under the addon directory since its last tag.
3. Choose the next SemVer from those changes. Bump patch for compatible fixes, minor for compatible
   features, and major for incompatible changes to configuration, behavior or host requirements.
   During initial development (0.x), bump minor for an incompatible change and document the migration.
   For the first tag of existing untagged content, use its manifest version if it still describes that content.
4. Set `version` in the manifest and rename the Unreleased heading to that version. If the first tag
   publishes an already documented version, use its existing heading. Run the checks in the addon's README,
   check a config using the addon, and update the README installation example with the Git URL,
   release tag, optional subdirectory and attachment a user can copy into their project. Include these
   files in the release commit.
5. To publish, run the following from the repository root with the addon's name, version and tested commit.
   Check that the manifest at that commit has this name and version, and that its changelog describes it.
   Publish the commit through the repository's usual workflow, then publish only this tag:

   ```sh
   tag="addons/$name/$version"
   git tag -a "$tag" "$commit" -m "$name $version"
   git push origin "refs/tags/$tag"
   git ls-remote --refs --tags origin "refs/tags/$tag"
   ```

Keep published tags on their original commits. A client caches the commit it first resolves for a tag.
Publish a new version for a correction. When moving an addon, keep old tags reachable at the old URL
so clean installs of existing pins still work. Document the new URL and relative directory for later versions.

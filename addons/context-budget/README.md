# context-budget

An addon of loop-manager for claude and codex agents: it bounds the context of a session. Past a soft limit the agent
is told to wrap up and leave its summary on the run's bead; past a hard limit the session is stopped and its state
goes on the bead. The context is never compacted. [Demo 4](../../demos/4-context-budget/README.md) runs it on a claude
agent. What differs on codex is in [On codex](#on-codex).

## Install

In your project's `loop-manager.toml`, declare the Git source and attach the addon to the project's agents.
If you already have a `[settings]` table, append the attachment to its `addons` array.

```toml
[[addon]]
git = "https://github.com/kvokka/loop-manager.git"
rev = "2278ccdcccba31f65560c8ed5b039036e0e65b01"  # context-budget 0.3.0
dir = "addons/context-budget"

[settings]
addons = [{ name = "context-budget" }]
```

Run `loop-manager config check`, then `loop-manager config reload` to apply the change on the project's daemon.
To enable the addon for a single agent, put the attachment in that agent's `agent.toml` and keep the Git
declaration in the project file.

## Params

| Param | Default | What |
|---|---|---|
| `soft` | `300000` | tokens of context past which the agent is told to wrap up |
| `hard` | `420000` | tokens of context past which the session is stopped |
| `warn_step` | `30000` | tokens the context grows past `soft` between two notes |
| `cut` | `"fail"` | the outcome a stopped session declares for its run: `"fail"` or `"pass"` |

The load checks only the type of a param. A `cut` other than `"fail"` or `"pass"` declares no outcome: the session is
stopped all the same, and the hook says so on stderr. A `hard` past about 967000 is past the compaction window the
addon sets (see `PreCompact` below), so there only the blocked `PreCompact` keeps claude from compacting. On codex
both limits are bounded by the window of the model: give a codex agent its own (see [On codex](#on-codex)).

## How it works

The hook `hooks/budget` runs in the agent's session on five hooks of the harness. This section is claude's; codex is
in the next one. The context of an agent is the size of the
last answer of its model in its transcript: input, cache writes, cache reads and output, the synthetic answers of
claude left out. A subagent's context is its own, from its transcript beside the session's
(`<session>/subagents/agent-<agent_id>.jsonl`, or under `$CLAUDE_CONFIG_DIR/projects`, `~/.claude/projects` without
it).

- `SessionStart` and `SubagentStart`: the agent learns its budget, `soft`, and that a note will tell it to wrap up.
  The session's first start in a run keeps the commit it starts on, for the dump.
- `PostToolUse`, past `soft`: a note joins the agent's context: its context and budget, then what to do. Start
  nothing new and call no subagents; finish the step at hand or roll it back; commit; leave the summary on the bead
  (`br comments add "$LOOP_MANAGER_BEAD"`: what is done, what did not work, what is started and not finished); end
  with `loop-manager finish`. A run without a bead puts the summary in `--summary`. A subagent is told to return its
  summary to its caller instead. The note comes again each time the context grows by `warn_step`.
- `PreToolUse`, past `soft`: a call of a subagent (the tool `Agent`) is denied.
- Past `hard`, on either tool hook: the state of the work goes on the bead as a comment: `git status --short`, the
  commits since the run's first session started, `git diff --stat`, the agent's last tool calls and last texts. The
  session declares the outcome `cut` (`loop-manager finish <cut>`) and stops: every tool call is denied from then on
  and the turn ends. Neither claude nor codex runs a `Stop` hook after that stop, so no `Stop` hook of another addon,
  as `lessons`, takes the session on. A subagent past its own `hard` stops alone, with the dump and no outcome: the
  session goes on, and its agent declares the outcome.
- `PreCompact`: blocked, so claude never compacts. The addon sets claude's `autoCompactWindow` to 1000000, the most
  claude takes, so that the window stays past `hard`.

The agent hears of `soft` only. Nothing the hooks say to it holds `hard`: the stop is a safety net that a session that
wraps up in time never meets. An outcome the finisher does not take (a daemon out of reach, a remark of a
`run.finishing` hook) is reported on the hook's stderr and the session stops all the same.

The addon's data directory keeps, per session, the context of the last note (`<session>.noted`), the dump
(`<session>.dump.md`) and the stop (`<session>.stop`); a subagent's files carry `.agent-<id>` after the session.
`<run>.base` is the commit the run's first session started on.

## On codex

The hook is the same; it knows a codex input by its `turn_id`. The context is `last_token_usage.total_tokens` of the
last `token_count` event of the session's rollout (`transcript_path`); a subagent is a child thread with a rollout of
its own, named by its input's `transcript_path`, and its tool is `spawn_agent`. What codex does not give:

1. **Compaction cannot be refused without a stop.** codex's `PreCompact` takes no `decision: block`. The addon sets codex's
   `model_auto_compact_token_limit` to `hard` and answers its attempt to compact with `continue: false`: there is no
   compaction, the turn ends and the session is stopped, with the dump and the outcome `cut`. The attempt to compact
   is the hard limit, counted by codex itself.
2. **The budget is bounded by the model's window.** codex compacts at 90% of the window at most, whatever the limit
   says (272000 for the models of its catalog, so 244800). A `hard` past that stops the session at 90% of the window,
   with the same dump and outcome; a `soft` past it gives no note at all. The defaults (300000 and 420000) do not fit
   a window of 272000: give a codex agent its own, as `params = { soft = 180000, hard = 240000 }`, or a larger window
   with `native.codex.model_context_window` where the model takes one.
3. **A tool hook does not end a turn.** codex's `PreToolUse` takes only a denial of the one call; `continue: false`
   there fails the hook and the call runs. Past `hard` every call is denied, and `PreCompact` ends the turn. Past
   `hard`, a `PostToolUse` answer of `continue: false` replaces the result of the tool with the reason.
4. **The measure is one model request late.** codex writes the usage of a request after its tools ran, so the note
   past `soft` comes one step later than on claude. `hard` is not affected: codex counts it.
5. **The note follows a successful call only.** codex runs no `PostToolUse` after a call that failed.
6. **A subagent** is a child thread with the compaction limit and hooks of its session. This holds on multi-agent v1;
   on v2 it is read from codex's source, not run.
7. **The turn ends `interrupted`.** loop-manager sees it as cancelled; the hook has declared the outcome by then. When
   the finisher does not take the outcome, the turn is stopped all the same and the run gets a turn with no outcome.

## Needs on the host

`jq`, `git`, `br` (the comments; it finds the tracker by the session's `BEADS_DIR`, as the agent's own calls do) and
`loop-manager` (the finisher) on the `PATH` of the harness's hooks.

See the [changelog](CHANGELOG.md) for version history.

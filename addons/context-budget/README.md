# context-budget

An addon of loop-manager for claude agents: it bounds the context of a session. Past a soft limit the agent is told
to wrap up and leave its summary on the run's bead; past a hard limit the session is stopped and its state goes on
the bead. The context is never compacted. [Demo 4](../../demos/4-context-budget/README.md) runs it on a claude
agent. There is no codex side yet: the addon declares `harnesses = ["claude"]`.

```toml
# the orchestrator's config.toml or a project's loop-manager.toml
[[addon]]
path = "<this directory>"            # or git = "<this repository>", rev = "<tag>", dir = "addons/context-budget"

# any settings layer: [settings], a rig, agent.toml, a variant
addons = [{ name = "context-budget", params = { soft = 200000, hard = 300000 } }]
```

## Params

| Param | Default | What |
|---|---|---|
| `soft` | `300000` | tokens of context past which the agent is told to wrap up |
| `hard` | `420000` | tokens of context past which the session is stopped |
| `warn_step` | `30000` | tokens the context grows past `soft` between two notes |
| `cut` | `"fail"` | the outcome a stopped session declares for its run: `"fail"` or `"pass"` |

The load checks only the type of a param. A `cut` other than `"fail"` or `"pass"` declares no outcome: the session is
stopped all the same, and the hook says so on stderr. A `hard` past about 967000 is past the compaction window the
addon sets (see `PreCompact` below), so there only the blocked `PreCompact` keeps claude from compacting.

## How it works

The hook `hooks/budget` runs in the agent's session on five claude hooks. The context of an agent is the size of the
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
  and the turn ends. A subagent past its own `hard` stops alone, with the dump and no outcome: the session goes on,
  and its agent declares the outcome.
- `PreCompact`: blocked, so claude never compacts. The addon sets claude's `autoCompactWindow` to 1000000, the most
  claude takes, so that the window stays past `hard`.

The agent hears of `soft` only. Nothing the hooks say to it holds `hard`: the stop is a safety net that a session that
wraps up in time never meets. An outcome the finisher does not take (a daemon out of reach, a remark of a
`run.finishing` hook) is reported on the hook's stderr and the session stops all the same.

The addon's data directory keeps, per session, the context of the last note (`<session>.noted`), the dump
(`<session>.dump.md`) and the stop (`<session>.stop`); a subagent's files carry `.agent-<id>` after the session.
`<run>.base` is the commit the run's first session started on.

## Needs on the host

`jq`, `git`, `br` (the comments; it finds the tracker by the session's `BEADS_DIR`, as the agent's own calls do) and
`loop-manager` (the finisher) on the `PATH` of the harness's hooks.

# Your first formula with an agent

This page adds an **agent** to the project of [the previous page](first-project.md) and a `prompt` formula that
gives it a task. Use the cheapest model while you try things out.

## 1. The agent

An agent is a directory under `agents/` with an `agent.toml` and, usually, a role prompt in `prompt.md`.
`~/work/hello/agents/helper/agent.toml`:

```toml
harness = "claude"                    # one of the [harness.*] of the orchestrator config
model = "haiku"                       # optional; the harness default otherwise
permission_mode = "acceptEdits"       # claude's --permission-mode; for codex its approval policy
```

`~/work/hello/agents/helper/prompt.md`:

```markdown
You are a careful assistant working in this repository.
```

Settings such as `model`, `env`, `mcp`, `hooks` and `skills` may also be set for the whole orchestrator, a project
or a rig (`[settings]`); the agent and the overrides are stronger layers. See what a session will actually get, and
which layer each value comes from:

```sh
aae config explain --project hello --agent helper
```

By default the harness also reads your own host settings (`~/.claude`, `~/.codex`); `inherit_host = false` turns
that off.

## 2. The formula

`~/work/hello/formulas/summarize.toml`:

```toml
name = "summarize"
description = "Write a one-paragraph summary of a file"

[inputs]
file = { type = "string", required = true }   # bead | string | integer | boolean

[step]
kind = "prompt"
agent = "helper"
prompt = "summarize.md"               # relative to this file
timeout = "10m"
```

`~/work/hello/formulas/summarize.md`, the prompt template:

```markdown
Read {{ inputs.file }} in {{ rig.path }} and write a one-paragraph summary of it to SUMMARY.md.
When you are done, run `aae finish pass --summary "<what you did, in one line>"`.
If you cannot do it, run `aae finish fail --summary "<why>"`.
```

The template is checked when the config loads: `inputs.<name>` must be a declared input, and `rig.*`,
`project.*`, `run.*` and `bead.*` must be fields that exist (see [the config reference](../design/config.md)).

## 3. How a run ends: the finisher

A prompt run does not pass because the agent stopped talking. It passes when the agent calls `aae finish pass`
from its session (the session's environment carries the run and an attempt token, so the call needs no arguments
besides the result and the summary). A turn that ends without a finish gets a reminder; one that still ends without
one fails the run. Tell the agent about `aae finish` in the prompt, as above, and make sure `aae` is on the `PATH`
the harness sees.

The agent must be allowed to run that command. With `permission_mode = "acceptEdits"` claude asks before running a
shell command; the run then waits in `requires_action` until you answer (or until the `human_timeout`, one hour
by default, after which the request is denied). The `run.requires_action` event names the request:

```sh
aae events --json | grep requires_action   # {"run": ..., "request": {"id": "<request>", "kind": "permission", ...}}
aae run answer run_2 <request> allow       # or: deny <message>; answer <text> for a question
```

## 4. Run it

```sh
aae config reload
aae run start hello main summarize --input file=README.md
aae run watch run_2                   # its events and the session's messages live; ends with the run
aae run show run_2
```

The run goes `queued` → `preparing` → `running` → `finishing` → `passed`. While it runs, `aae instance list` shows
the harness process serving it. `aae run cancel <run>` stops it.

## A worktree per run

With `worktree = true` next to `name` in the formula, each run works in its own git worktree of the rig, on the
branch `aae/<run>`, so runs of one rig do not step on each other. aae makes it before the run starts and removes it
when the run ends, unless it holds uncommitted changes or commits no ref reaches; the branch goes only once merged.
The rig must be a git repository. Without the key a run works in the rig itself.

## Runs over a tracker

A project with a `.beads` directory uses [br](../../README.md#the-br-tracker-and-its-license) as its task
tracker. A formula input of `type = "bead"` takes a bead id, or `{ next_ready = true }` to take the next ready bead;
the prompt then reads its fields as `{{ bead.title }}`, `{{ bead.description }}` and so on, and the run holds the
bead's claim until it ends. A trigger on `bead.ready` starts such runs as beads become ready.

Next: [writing a plugin](plugin.md).

# Your first formula with an agent

This page adds an **agent** to the project of [the previous page](first-project.md) and a `prompt` formula: the agent
is a general role, the formula the process it goes through, and the task itself is the run's input, from you.

## 1. The agent

An agent is a directory under `agents/` with an `agent.toml` and, usually, a role prompt in `prompt.md`.
`~/work/hello/agents/helper/agent.toml`:

```toml
provider = "anthropic"                # the harness follows from it: claude (or name `harness = "claude"`)
model = "sonnet"                      # optional; the harness default otherwise; a short name is the latest model
effort = "medium"                     # optional
permission_mode = "acceptEdits"       # claude's --permission-mode; for codex its approval policy
```

`~/work/hello/agents/helper/prompt.md`:

```markdown
You are a careful assistant working in this repository.
```

`provider`, `harness`, `model` and `effort` in `agent.toml` are only the role's defaults. What the agent runs on in
your project is your setting, stronger than the agent's: an override for the whole project or one rig, here the
cheapest model while you try things out, in `~/work/hello/aae.toml`:

```toml
[overrides.helper]                    # or [rigs.main.overrides.helper] for one rig
model = "haiku"
```

To run the same role on another provider under a name of its own, declare a variant (`[agents.<name>] base =
"<pack>.<agent>"`) and map it on a rig (`[rigs.<r>] agents = { helper = "<name>" }`); see
[Agent variants](../design/config.md#agent-variants). Settings such as `env`, `mcp`, `hooks` and `skills` may also be
set for the whole orchestrator, a project or a rig (`[settings]`), weaker than the agent. See what a session will
actually get, and which layer each value comes from:

```sh
aae config explain --project hello --agent helper
aae config explain --project hello --formula summarize --rig main   # with the formula's and the rig's layers
```

By default the harness also reads your own host settings (`~/.claude`, `~/.codex`); `inherit_host = false` turns
that off.

A codex agent runs with `sandbox_mode = "danger-full-access"` unless its `[native.codex]` table (or your
`~/.codex/config.toml`) sets `sandbox_mode`: its commands can touch anything your user can. `aae doctor` names
each such agent in a warning; set `sandbox_mode = "workspace-write"` where the host allows user namespaces.

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

`~/work/hello/formulas/summarize.md`, the prompt template: the steps of the work, with the inputs filled in.

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
by default, after which the request is denied). `aae run show` names the open request:

```sh
aae run show run_2                         # request.id: <request>, request.kind: permission, request.tool: Bash
aae run answer run_2 <request> allow       # or: deny <message>; answer <text> for a question
```

## 4. Run it

```sh
aae config reload
aae run start hello main summarize --input file=README.md
aae run watch run_2                   # its events and the session's messages live; ends with the run
aae run show run_2
```

An `--input name=value` value is text, read by the type the formula gives the input: `version=1.10` stays the string
`1.10` for a `string` input, and becomes a number or a boolean only for an `integer` or `boolean` one.

The run goes `queued` → `preparing` → `running` → `finishing` → `passed`. While it runs, `aae instance list` shows the
harness process serving it. `aae run cancel <run>` stops it.

A run that stays `queued` waits for something: `aae run show` names it in `waiting_for`. One wait surprises at first:
a step that fails with a fault of the rig rather than of the work — a `command` that does not exist (a typo), a
working directory that cannot be used, `workspace_failure` in `aae events` — closes the rig's gate
(`rig:<project>/<rig>`), and every later run of the rig waits until you fix the cause and run
`aae gate open rig:hello/main`. `aae gate list` shows the closed gates and why.

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

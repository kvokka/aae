# Your first formula with an agent

This page adds an **agent** to the project of [the previous page](first-project.md) and an `ai` formula: the agent
is a general role, the formula the process it goes through, and the task itself is the run's input, from you.
Keep it that way: an agent's prompt knows nothing of a task and a pack holds no word of one; what to do comes
only from the input of a run.

## 1. The agent

An agent is a directory under `agents/` with an `agent.toml`, the prompt template its `ai` steps get in
`prompt.template.md` (next section) and, usually, a role prompt in `prompt.md`.
`~/work/hello/agents/helper/agent.toml`:

```toml
harness = "claude"                    # the agent CLI, `claude` or `codex`
model = "sonnet"                      # optional; the harness default otherwise; a short name is the latest model
effort = "medium"                     # optional
```

`~/work/hello/agents/helper/prompt.md`:

```markdown
You are a careful assistant working in this repository.
```

`harness`, `model` and `effort` in `agent.toml` are only the role's defaults. What the agent runs on in
your project is your setting, stronger than the agent's: an edit of the agent for the whole project or one rig, here the
cheapest model while you try things out, in `~/work/hello/aae.toml`:

```toml
[agents.helper]                       # or [rigs.<r>.agents.helper] for one rig of a project with rigs
model = "haiku"
```

To run the same role on another harness under a name of its own, declare a variant (`[agents.<name>] base =
"<pack>.<agent>"`) and map it on a rig (`[rigs.<r>] agents = { helper = "<name>" }`); see
[Agent variants](../design/config.md#agent-variants). Settings such as `env`, `mcp`, `hooks` and `skills` may also be
set for the whole orchestrator, a project or a rig (`[settings]`), weaker than the agent. See what a session will
actually get, and which layer each value comes from:

```sh
aae config reload                                        # the daemon takes the new files; explain asks the daemon
aae config explain --project hello --agent helper
aae config explain --project hello --formula summarize   # with the formula's layers, in the root (`--rig` for a rig)
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
file = { type = "string", required = true }   # task | string | integer | boolean

[[step]]
kind = "ai"
agent = "helper"                      # the step gets the agent's prompt template
timeout = "10m"
```

`~/work/hello/agents/helper/prompt.template.md`, the prompt template: the steps of the work, with the inputs filled in.

```markdown
Read {{ inputs.file }} in {{ rig.path }} and write a one-paragraph summary of it to SUMMARY.md.
When you are done, run `aae finish pass --summary "<what you did, in one line>"`.
If you cannot do it, run `aae finish fail --summary "<why>"`.
```

An agent has one template, and it is checked when the config loads against every formula whose `ai` steps name
the agent: `inputs.<name>` must be an input each of them declares, `bead.*` needs a `task` input, `steps.<name>.*`
an earlier step, and `rig.*`, `project.*` and `run.*` must be fields that exist (see
[the config reference](../design/config.md)). Formulas that need different prompts name different agents.

A project changes the prompt of a pack's agent without touching the pack: its own
`.aae/agents/<agent>/prompt.template.md`, next to `aae.toml`, replaces the pack's. A template also takes in other
files of its agent by name: `{{ agent.style }}` is the text of `style.md`, the project's
`.aae/agents/<agent>/style.md` over the pack's `agents/<agent>/style.md`. That is how a project gives a general agent
its own text, say the conditions a checker tries; a file found in neither place is a config error naming the path.

## 3. How a run ends: the finisher

An `ai` step does not pass because the agent stopped talking. It passes when the agent calls `aae finish pass`
from its session (the session's environment carries the run and an attempt token, so the call needs no arguments
besides the result and the summary). A turn that ends without a finish gets a reminder; one that still ends without
one fails the run. Tell the agent about `aae finish` in the prompt, as above, and make sure `aae` is on the `PATH`
the harness sees.

The agent must be allowed to run that command. By default it is: claude runs in `bypassPermissions` and codex with
`approval_policy = "never"`. Nothing waits for a human: what the config does not allow is denied at once, and the agent
sees the denial. A stricter claude mode must allow the call explicitly:

```toml
[native.claude]
permission_mode = "acceptEdits"
permissions = { allow = ["Bash(aae finish *)"] }
```

Under a codex `approval_policy` other than `never`, a command that needs approval is declined. More in
[config](../design/config.md#permissions).

## 4. Run it

```sh
aae config reload
aae run start hello summarize --input file=README.md
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
`aae gate open rig:hello/root`. `aae gate list` shows the closed gates and why.

## Several steps

A formula may list several `[[step]]`s; a run goes through them in order, and the first one that fails ends it. A
step is named by `name`, or by its position (`"1"`, `"2"`, ...), and sees the outcomes of the steps before it.
`~/work/hello/formulas/summarize-checked.toml`:

```toml
name = "summarize-checked"
description = "Summarize the README and check the summary is there"

[inputs]
file = { type = "string", required = true }   # helper's template reads {{ inputs.file }}

[[step]]
name = "count"
kind = "exec"
command = ["wc", "-l", "README.md"]

[[step]]
name = "write"
kind = "ai"
agent = "helper"                      # the template it shares with `summarize` cannot read {{ steps.count.* }}

[[step]]
name = "check"
kind = "exec"
command = ["test", "-s", "SUMMARY.md"]  # gets AAE_STEP_COUNT_* and AAE_STEP_WRITE_* in its environment
```

A prompt template reads an earlier step as `{{ steps.<name>.result }}` (`pass` or `fail`), `.summary` and
`.data.<key>`; an exec step's `data` is `{exit_code, stdout, stderr}`. An exec step gets the same as
`AAE_STEP_<NAME>_RESULT`, `AAE_STEP_<NAME>_SUMMARY` and `AAE_STEP_<NAME>_DATA` (JSON). Each ai step has a session
of its own. `aae run show` names the step the run is at, and `run.step_finished` marks each ended step but the
last.

A check can send the work back instead of failing the run: with `back_to = "write"` on the step `check`, a failed
check starts `write` again, and its prompt can show why with `{{ remarks }}` (empty the first time). `rounds = 2`
among the formula's own keys, next to its `name` above the steps, caps the rounds; without it the formula takes the `rounds` of `aae.toml`, else of the orchestrator config, else 3, and a check still failing in the last one fails the run with its
remarks as the reason. `config explain` goes step by step:

```sh
aae config reload
aae config explain --project hello --formula summarize-checked
# rounds = 2  # formula summarize-checked
# step `count` of `summarize-checked`: exec `wc -l README.md`
# step `write` of `summarize-checked` on `root`: agent hello.helper
# ...one line per value with its layer, e.g. model = "haiku"  # project [agents]
# step `check` of `summarize-checked`: exec `test -s SUMMARY.md`
```

## A worktree per run

With `worktree = true` next to `name` in the formula, each run works in its own git worktree of the rig, on the
branch `aae/<run>`, so runs of one rig do not step on each other. aae makes it before the run starts and removes it
when the run ends, unless it holds uncommitted changes or commits no ref reaches; the branch goes only once merged.
The rig must be a git repository. Without the key a run works in the rig itself.

## Runs over a tracker

A project with a `.beads` directory uses [br](../../README.md#the-br-tracker-and-its-license) as its task
tracker. A formula input of `type = "task"` takes the task: its text, the id of a bead to take up, or
`{ next_ready = true }` to take the next ready bead;
the prompt then reads its fields as `{{ bead.title }}`, `{{ bead.description }}` and so on, and the run holds the
bead's claim until it ends. A trigger on `bead.ready` starts such runs as beads become ready.

A task in plain words, `--input 'task=Add a dark mode'`, creates a root bead (an epic titled by the first line) and
the run works on it; a task that is the id of a bead files nothing and takes that bead. A planner step files the work as beads under that
root, each under the id prefix of its rig (`BD_ISSUE_PREFIX=<prefix> br create ... --deps parent-child:<root>`) and, if it
wants, with `agent:` and `harness:` labels; a `drain` step then gives every
ready bead under the root a run of its own formula and ends when none is ready or running:

```toml
name = "deliver"
[inputs]
task = { type = "task", required = true }

[[step]]
name = "plan"
kind = "ai"
agent = "planner"                     # its template files the beads under {{ bead.id }}

[[step]]
name = "work"
kind = "drain"
formula = "implement"                 # takes one `task` input; one run per bead, on the rig of its id prefix
pool = 2                              # two beads given at once (default 1); with no `worktree = true`
                                      # in `implement` they share the rig and work one after the other
on_item_failure = "continue"          # a failed bead stops nothing; soft_stop / hard_stop stop the step

[[step]]
name = "verify"
kind = "exec"
command = ["make", "check"]           # sees AAE_STEP_WORK_DATA: {closed, failed, refused}
back_to = "plan"                      # a failure sends the run back to `plan`, whose prompt shows {{ remarks }}
```

A step can also repeat itself before it gives up, and a script can judge its result:

```toml
[[step]]
name = "write"
kind = "ai"
agent = "helper"
max_attempts = 3                      # up to three executions; the repeat sees the last remark in {{ remarks }}
step_check_script_path = "check.sh"   # beside the formula file; 0 passes, 75 fails for good, else repeats
```

Exit code 75 of the script is a failure for good: the run fails at once, with no repeat and no `back_to` round.

A step can run another formula of the project as a child run, whose outcome is the step's; the child inherits the
inputs it declares (a `task` too: the child works on the bead of the parent, which closes it), and the step
adds its own:

```toml
[[step]]
name = "review"
kind = "formula"
formula = "review"                    # with `internal = true` in review.toml it starts only from a step
inputs = { focus = "{{ steps.write.summary }}" }
```

Next: [using plugins](plugins.md) and [writing a plugin](plugin-development.md).

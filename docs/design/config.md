# Configuration files

The files the daemon reads (spec §6, §8, §9.1, §9.5, §11) and the rules the loader enforces. Layer merging and
"explain config" are a separate step (§6.3); this document fixes what one file may say. Durations are strings
with one unit, as in [`contracts.md`](contracts.md) (`"500ms"`, `"30s"`, `"45m"`, `"2h"`). A relative path is
relative to the directory of the file that holds it; a leading `~/` is the home directory. Unknown keys are an
error in every file.

## Loading

- The daemon loads everything at once from the orchestrator file: the projects it lists, their packs, agents,
  formulas, triggers and the plugins they declare. A missing orchestrator file is an empty config.
- Load is all or nothing: every error found is reported with its file, and an invalid config is never applied.
  The daemon re-reads only on an explicit command (§6.5); on error the previous config stays in force.

## Settings (any layer)

One table shape for every layer of §6.3: `[settings]` of the orchestrator, a project and a rig; the `[agent]`
contribution of a plugin; the top level of `agent.toml`; and the overrides `[overrides.<agent>]`.

| Key | Type | Merge (§6.3) |
|---|---|---|
| `model`, `effort`, `system_prompt`, `permission_mode` | string; `permission_mode` is the harness's own: claude's `--permission-mode`, codex's approval policy (`untrusted`, `on-request`, `never`) | scalar |
| `inherit_host` | boolean | scalar |
| `env` | table of strings | by key |
| `mcp.<name>` | `{command = [..], env}` or `{url, headers}`; optional `scope = "orchestrator" \| "project" \| "session"` | by key, entry replaced whole |
| `hooks.<name>` | `{event, matcher, command, timeout}` (harness hooks, Claude dialect, §11.2) or `{enabled = false}` | named list |
| `skills.<name>` | `{path}` (must exist) or `{enabled = false}` | named list |
| `plugins` | array of `{name, enabled = true, config = {..}}`, in attachment order | named list |
| `native.<harness>` | any table, passed to the adapter as is; `native.claude` is a table of claude settings keys, delivered with `--settings` together with the hooks; `native.codex` is a table of codex `config.toml` keys, delivered to the app-server as `-c key=value` flags (so it is part of the process fingerprint) | by key |

## Effective config and "explain config"

`Config::effective(project, rig, agent)` merges the layers of §6.3 for one session: the harness default model,
orchestrator, project and rig settings, then the contributions of the attached plugins in attachment order, the
agent, and the overrides of the orchestrator, the project and the rig. Plugins attach first (the `plugins` key of
every layer), so a stronger layer can detach a plugin and its whole contribution. An override applies when its key
resolves (see Names) to the agent. Every value keeps its layer; `explain()` prints one line per value with its
layer and one line for the host: inherited by the harness and invisible to aae, or off. The effective config
serializes to JSON: a run keeps it as its snapshot, and `env` is the whole session environment besides what the
adapter adds (§6.5). The adapter's `split` turns it into a process part, keyed by its fingerprint (contracts 8),
and a session part.

## Orchestrator: `~/.config/aae/config.toml`

```toml
projects = ["~/work/shop"]          # project directories, each with an aae.toml
max_runs = 8                        # runs started at once across all projects (default 8, at least 1)

[harness.claude]                    # harnesses agents may choose (§6.1)
model = "sonnet"                    # default model of the harness
binary = "/opt/claude/bin/claude"   # optional explicit path; otherwise resolved through mise (§6.5)

[[plugin]]                          # an installed plugin; its orchestrator side is enabled
path = "~/aae-plugins/aae-plugin-mail"

[settings]                          # layer 2
[overrides.implementer]             # layer 7, for an agent of any project
```

## Project: `<project>/aae.toml`

```toml
name = "shop"                       # optional, defaults to the directory name; unique among projects
packs = ["../packs/gascity"]        # local pack directories

[[plugin]]
path = "plugins/aae-plugin-notes"

[settings]                          # layer 3
[overrides.implementer]             # layer 7

[rigs.backend]
path = "../backend"                 # must exist
vars = { test_cmd = "cargo test" }
triggers = ["nightly-audit"]        # pack triggers that run on this rig (a pack does not know rig names, §9.5)
[rigs.backend.settings]             # layer 4
[rigs.backend.overrides.implementer]

[[trigger]]                         # project triggers may name their rig
name = "audit-backend"
cron = "0 3 * * *"
run = "audit"
rig = "backend"
```

The project root is itself a pack named after the project: `agents/` and `formulas/` next to `aae.toml` belong to
it, and its triggers are the `[[trigger]]` of `aae.toml`.

## Pack: `<pack>/pack.toml`

```toml
name = "gascity"
version = "0.3.0"
requires_plugins = ["mail"]         # must be declared by the orchestrator or the project
min_aae = "0.1.0"                   # optional

[[trigger]]
name = "implement-next"
on = "bead.ready"
where = { "bead.labels" = "auto" }
run = "implement"
inputs = { bead = { next_ready = true, epic = "{{ event.bead.epic }}" } }
```

Content: `agents/<agent>/agent.toml` (plus `prompt.md` and any other files of the agent, §8.1) and
`formulas/*.toml`.

## Names

- Pack, agent, formula, trigger, rig and project names are non-empty and have no `.`; agent, formula and trigger
  names are unique within a pack, pack names within a project.
- A reference is `<name>` or `<pack>.<name>`. A bare name resolves to the referring pack's own item first, then to
  the only pack that has it; a name that two other packs share must be qualified.

## Agent: `agents/<agent>/agent.toml`

`harness` (required; one of the orchestrator's `[harness.*]`), `prompt` (role prompt file, default `prompt.md`
when it exists), `max_runs` (optional positive integer: runs of this agent started at once in its project; the
orchestrator's `max_runs` still applies), then the settings keys above.

A run counts toward both limits from `preparing` until it is terminal, except while `parked` (no harness process).
A run over a limit waits in `queued` with `waiting_for = "concurrency"`; a busy workdir is reported first.

## Formula: `formulas/<file>.toml`

```toml
name = "implement"
description = "Implement the task of a bead"

[inputs]
bead = { type = "bead", required = true }   # bead | string | integer | boolean

[step]
kind = "prompt"                     # prompt: agent + prompt template; exec: command
agent = "implementer"
prompt = "implement.md"             # relative to the formula file
timeout = "45m"                     # optional
```

An `exec` step has `command = ["prog", "arg", ..]` (non-empty) and `timeout` instead of `agent` and `prompt`.

A prompt template is checked at load (contracts 2.3): `inputs.<name>` must be a declared input; `bead.<field>` needs
a `bead` input and a field of `Bead` (contracts 5.2); `run.<field>` a field of `RunView` (1.3); `project.name`,
`project.path`; `rig.name`, `rig.path` and `rig.<var>` (a rig variable, checked when the run starts).

## Trigger

`name`, `run` (a formula reference) and exactly one source: `on` (an event type of contracts 1.3, with optional
`where`), `cron` (five fields) or `every` (a duration). `inputs` follows contracts 2.3: every key is an input of the
formula, every required input is given, placeholders exist only with `on` and their paths exist in that event's
payload, as do the paths of `where`. `rig` is allowed only in a project's `[[trigger]]`.

## Plugin: `<plugin>/plugin.toml`

```toml
name = "mail"
version = "0.1.0"

[agent]                             # layer 5 contribution to the agents that attach the plugin (§11.2)
skills.mail = { path = "skills/mail" }

[[hook]]                            # orchestrator hook (§11.3, contracts 3)
on = "run.finishing"
command = ["bin/check-mail"]
timeout = "10s"                     # optional
```

Plugin names are unique across the orchestrator and the project; an attachment in `plugins` must name a declared
plugin.

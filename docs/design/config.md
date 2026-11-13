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
  The daemon re-reads only on an explicit command (§6.5): `aae config reload` (`POST /v1/config/reload`); on
  error the previous config stays in force, and `aae config check` reports the errors without applying anything.
- A reload is taken by new runs and by the schedules (a schedule whose `cron`/`every` changed is set again from the
  reload; a removed one is dropped); running runs keep the snapshot they prepared with; an idle instance stops with
  reason `config` only when no agent of the new config, on any rig, starts sessions on its fingerprint (a changed MCP
  restarts only the instances it is part of). It emits `config.reloaded` with the generation (1 at daemon start).
  A change of the set of harnesses, a harness `binary` or `max_sessions` (a harness `model` is fine), or of which
  projects have a `.beads` tracker is refused: drivers and trackers are made at daemon start.

## Settings (any layer)

One table shape for every layer of §6.3: `[settings]` of the orchestrator, a project and a rig; the `[agent]`
contribution of a plugin; the top level of `agent.toml`; and the overrides `[overrides.<agent>]`.

| Key | Type | Merge (§6.3) |
|---|---|---|
| `model`, `effort`, `system_prompt`, `permission_mode` | string; `permission_mode` is the harness's own: claude's `--permission-mode`, codex's approval policy (`untrusted`, `on-request`, `never`) | scalar |
| `inherit_host` | boolean | scalar |
| `env` | table of strings | by key |
| `mcp.<name>` | `{command = [..], env}` (stdio), `{url, headers}` (HTTP), or both: a service (below); optional `scope = "orchestrator" \| "project" \| "session"` | by key, entry replaced whole |
| `hooks.<name>` | `{event, matcher, command, timeout}` (harness hooks, Claude dialect, §11.2) or `{enabled = false}` | named list |
| `skills.<name>` | `{path}` (must exist) or `{enabled = false}` | named list |
| `plugins` | array of `{name, enabled = true}`, in attachment order; parameters belong to the `[[plugin]]` declaration | named list |
| `native.<harness>` | any table, passed to the adapter as is; `native.claude` is a table of claude settings keys, delivered with `--settings` together with the hooks; `native.codex` is a table of codex `config.toml` keys, delivered to the app-server as `-c key=value` flags (so it is part of the process fingerprint) | by key |

### MCP servers and services (spec §11.4)

- `url` alone: an HTTP server someone else runs. Every session gets it with its own `headers` and the identity
  headers `X-Aae-Project`, `X-Aae-Rig`, `X-Aae-Agent` and `X-Aae-Run`.
- `command` alone: a stdio server, started by the harness in each session. There is no stdio→HTTP bridge in this
  release, so a stdio server is per session whatever its `scope`; `aae doctor` warns about those not of scope
  `session`.
- `command` and `url`: a **service**. The daemon runs `command` once per scope (`orchestrator`, the default, or
  `project`; `session` is an error), lazily, when the first session that names it starts, and stops it 5 minutes
  after its last session ended. The process gets a clean environment with `env`, `AAE_SERVICE_PORT` (a free port
  on 127.0.0.1 picked by the daemon) and `AAE_SERVICE_TOKEN`; it must listen on that port and refuse requests
  without `Authorization: Bearer <token>`. `${AAE_SERVICE_PORT}` in `url` is replaced with the port, and sessions
  get the URL with the token and the identity headers. The service is up once its port accepts connections (that is
  the whole health check: no health command is declared, and a live service is one whose process lives); a
  session start waits for that up to 30 s, and a server that is not up is a tool failure, not a failed run. A
  service that dies is started again on the same port (with the effect backoff) while sessions use it, so they
  reconnect; with none, it is dropped. A changed definition is a new service; the old one idles out. Events:
  `service.starting`, `service.ready`, `service.failed`, `service.stopped`; `aae instances` lists services too.
- `url` with `${AAE_API}`: the daemon's own API on TCP, e.g. `url = "${AAE_API}/mcp"` for its MCP frontend. The
  placeholder becomes `http://<api_tcp>` and the session gets `Authorization: Bearer <token>` with the token of the
  daemon's start. With no `api_tcp` the session goes without the server, as with a service that is not up.

## Effective config and "explain config"

`Config::effective(project, rig, agent)` merges the layers of §6.3 for one session: the harness default model,
orchestrator, project and rig settings, then the contributions of the attached plugins in attachment order, the
agent, and the overrides of the orchestrator, the project and the rig. Plugins attach first (the `plugins` key of
every layer), so a stronger layer can detach a plugin and its whole contribution. An override applies when its key
resolves (see Names) to the agent. Every value keeps its layer; `explain()` prints one line per value with its
layer and one line for the host: inherited by the harness and invisible to aae, or off (`aae config explain
--project <p> --agent <a> [--rig <r>]`, `GET /v1/config/explain`). The effective config
serializes to JSON: a run keeps it as its snapshot, and `env` is the whole session environment besides what the
adapter adds (§6.5). The adapter's `split` turns it into a process part, keyed by its fingerprint (contracts 8),
and a session part.

## Orchestrator: `~/.config/aae/config.toml`

```toml
projects = ["~/work/shop"]          # project directories, each with an aae.toml
max_runs = 8                        # runs started at once across all projects (default 8, at least 1)
bead_failures = 3                   # failed runs of a bead before the core closes its gate (default 3, at least 1)
api_tcp = "127.0.0.1:7171"          # the API on TCP too (off by default; loopback only; a reload cannot change it)

[harness.claude]                    # harnesses agents may choose (§6.1)
model = "sonnet"                    # default model of the harness
binary = "/opt/claude/bin/claude"   # optional explicit path; otherwise resolved through mise (§6.5)

[harness.codex]
max_sessions = 8                    # codex only: threads per app-server before a second one starts (default 8)

[[plugin]]                          # an installed plugin; its orchestrator side is enabled
path = "~/aae-plugins/aae-plugin-mail"

[builtin.default-policy]            # a bundled plugin (see below); on with no table
backoff_cap_s = 300

[builtin.worktree]                  # off with no table; a table with parameters turns it on
agents = ["implementer"]

[settings]                          # layer 2
[overrides.implementer]             # layer 7, for an agent of any project
```

## Project: `<project>/aae.toml`

```toml
name = "shop"                       # optional, defaults to the directory name; unique among projects
packs = [
  "../packs/gascity",               # a local pack directory
  { git = "https://github.com/acme/aae-pack-review", rev = "v1.2.0" },  # a pack from git: branch, tag or commit
]

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

### Packs and plugins from git: `aae.lock` and `aae fetch` (spec §8.2, §11.6)

A pack in `packs` and a `[[plugin]]` (in the orchestrator file or a project's) is a local directory, or a git
repository with `pack.toml` / `plugin.toml` at its root: `{ git = "<url>", rev = "<branch|tag|commit>" }` for a pack,
`git = ".."` and `rev = ".."` instead of `path` for a plugin (exactly one of `path` and `git`; `rev` goes with `git`).

`aae fetch [--config <file>] [--update]` runs without a daemon. For every git source of the orchestrator file and
its projects' `aae.toml` it pins the revision to a commit in `aae.lock`, next to the file that declares the source,
and checks that commit out (detached) in `.aae/git/<commit>/` next to it:

```toml
[[source]]
git = "https://github.com/acme/aae-pack-review"
rev = "v1.2.0"                      # as written in the config
commit = "4f1c…"                    # the full commit id it named when fetched
```

A pinned source keeps its commit; only `--update` resolves the revision again. Pins of sources no longer declared
are dropped. It prints `<url> <rev> -> <commit> (fetched|kept|updated)` per source and exits non-zero when any
source fails (that source keeps its old pin). Loading the config never runs git and never moves a pin, so there are
no automatic updates: a git source with no pin in `aae.lock`, or with no checkout of its pin, is a config error that
says to run `aae fetch`. Commit `aae.lock` with the project; `.aae/` is a cache to ignore.

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

- A trigger runs on the rigs that list it in `triggers` and, for a project trigger, on its `rig`; one that no rig
  names never fires. An event trigger sees the events whose `project` and `rig` are its own or null.
- `cron` is read in UTC: minute, hour, day of month, month, day of week; each field is `*`, a value, `a-b`, any of
  them with `/step`, or a comma list; months and days also take `jan`..`dec` and `sun`..`sat`, and day 7 is Sunday.
  When both day fields are restricted, a day matching either fires (as in Vixie cron).
- `every` first fires one interval after the daemon first sees the trigger, then one interval after each firing.
  A schedule missed while the daemon was down fires once at start, and the next one is counted from then.
- A trigger on `trigger.skipped` is a config error: a skip that fires a trigger could skip again forever.
- A run started by a trigger is one level deeper than the run of its event (1 without one, a schedule included);
  past depth 10 the firing is skipped with `depth`. Loops are bounded by this depth, not by the length of a chain:
  a long epic is legal. Repeated runs over one bead are bounded by the bead's failure count (`bead_failures`).

### How runs start and what a firing guarantees

A run starts in one of three ways, all in the core: an explicit call (`POST /v1/runs`, `aae run start`, the MCP tool
`start_run`), an event subscription (`on`) or a schedule (`cron`, `every`). A trigger can also be fired by hand
(`POST /v1/triggers/fire`, `aae trigger fire`) and switched off and on per rig by the operator
(`POST /v1/triggers/disable|enable`, `aae trigger disable|enable`, event `trigger.switched`); a trigger that is off
fires neither on its schedule nor on events, only by hand, and stays off across restarts.

- A firing is idempotent on `(trigger, event seq)`. Triggers match in the transaction that commits the event, not
  on the JSONL journal, so the new run (or its `trigger.skipped`) commits with the event or not at all: a `kill -9`
  of the daemon neither loses nor doubles a firing (contracts §3.5).
- Overlap: while a run started by the same trigger with the same inputs is alive, a new firing is skipped with
  `trigger.skipped{overlap}`.
- A run started by a trigger inherits the correlation of its event. A tracker event carries the correlation of the
  run whose attempt made the write, known from the br actor `aae:<run>[:<attempt>]`; a write by a human starts a
  new chain (contracts §5).
- Tracker events are `bead.created`, `bead.updated`, `bead.closed` and `bead.ready`; `bead.ready` is raised when a
  bead first enters the ready set: open, unclaimed, every blocker closed.
- The event schema and its version, the `where` and `inputs` language, hook decisions and timeouts and the outbox
  lifetime are fixed in `docs/design/contracts.md`.

## Plugin: `<plugin>/plugin.toml`

```toml
name = "mail"
version = "0.1.0"
prompt = "prompt.md"                # optional: its text joins the system prompt of the attached agents

[agent]                             # layer 5 contribution to the agents that attach the plugin (§11.2)
skills.mail = { path = "skills/mail" }

[[hook]]                            # orchestrator hook (§11.3, contracts 3)
on = "run.finishing"
command = ["bin/check-mail"]
timeout = "10s"                     # optional
```

Plugin names are unique across the orchestrator and the project; an attachment in `plugins` must name a declared
plugin. A declaration (a `path`, or a `git` source as for packs above) may carry parameters,
`[[plugin]] path = ".." config = { .. }`: its orchestrator hooks get them
in `AAE_PLUGIN_PARAMS` (contracts 3.1). In the `[agent]` contribution, `${AAE_PLUGIN_DIR}` in a harness hook
`command` or an MCP `command` is replaced with the plugin's directory at load, so they reach the plugin's own files;
skill paths are relative to it. Beyond its `[agent]` layer a plugin gives its attached agents two things, in
attachment order: the text of its `prompt` file, after the system prompt the layers give (each fragment after a blank
line); and its `bin/` directory, when it has one, ahead of `PATH` (the `PATH` a layer sets, or else the daemon's).
The daemon's `PATH` taken this way becomes part of the effective config: it is in the run's snapshot and in the
instance fingerprint, so after a daemon restart with another `PATH` these agents get new instances.
The example `examples/aae-plugin-review` shows both sides.

## Bundled plugins: `[builtin.<name>]`

The plugins bundled with aae (spec §11.5) run in process as hooks of the contract of contracts 3, after every
declared plugin, so a declared plugin decides first. `enabled` (boolean) turns one on or off; every other key is a
parameter, checked when the daemon starts. A table with parameters and no `enabled` turns the plugin on. A
project's `aae.toml` may have `[builtin.<name>]` tables too: for the runs of that project such a table replaces the
orchestrator's table of that plugin whole (spec §11.3: orchestrator-side plugins are turned on for the orchestrator
or for a project).

- `default-policy` (on unless `enabled = false`) decides on `run.attempt_failed` and
  `run.turn_ended_without_outcome` as in contracts 10. Parameters, all integers: `quota_margin_s` (60),
  `quota_wait_s` (1800), `model_pause_s` (600), `backoff_base_s` (10), `backoff_cap_s` (300), `backoff_park_from`
  (3), `rapid_s` (30), `crash_loop` (5), `crash_pause_s` (300), `resume_failures` (3), `unknown_after_s` (30),
  `unknown_retries` (3).
- `worktree` (off) gives each run of the agents in `agents` and the formulas in `formulas` (bare or qualified names;
  both empty: every run) a git worktree of its rig on the branch `aae/<run>`, in `<git common dir>/aae/worktrees/<run>`,
  as its workdir. When the run ends the worktree is removed unless it has uncommitted changes or a HEAD no ref
  reaches (then `plugin.hook_failed` says why); the branch is deleted only when it is merged. A run with
  `continue_from` starts in the workdir of the run it continues (a session resumes where it ran); when that is a
  worktree of this plugin, it is brought back on its branch if it was removed, whether or not the run is chosen.

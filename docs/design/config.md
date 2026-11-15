# Configuration files

The files the daemon reads, the rules the loader enforces, and how the layers merge into the config of one session
(see "Effective config and "explain config"" below). Durations are strings
with one unit, as in [`contracts.md`](contracts.md) (`"500ms"`, `"30s"`, `"45m"`, `"2h"`). A relative path is
relative to the directory of the file that holds it; a leading `~/` is the home directory. Unknown keys are an
error in every file.

## Loading

- The daemon loads everything at once from the orchestrator file: the projects it lists, their packs, agents,
  formulas, triggers and the plugins they declare. A missing orchestrator file is an empty config.
- Load is all or nothing: every error found is reported with its file, and an invalid config is never applied.
  The daemon re-reads only on an explicit command: `aae config reload` (`POST /v1/config/reload`); on
  error the previous config stays in force, and `aae config check` reports the errors without applying anything
  (it loads the file itself, so it needs no daemon).
- A reload is taken by new runs and by the schedules (a schedule whose `cron`/`every` changed, and every `cron` when
  `timezone` changed, is set again from the reload; a removed one is dropped); the snapshot is per step: a running
  step keeps the one it prepared with, and the next step of the run prepares with the config of that moment; an idle instance stops with reason `config` only when no agent of the new config, on any rig, starts
  sessions on its fingerprint (a changed MCP restarts only the instances it is part of). It emits `config.reloaded`
  with the generation (1 at daemon start). A change of the set of harnesses, a harness `binary` or any adapter setting
  such as `max_sessions` (a harness `model` is fine), or of which projects have a `.beads` tracker is refused: drivers
  and trackers are made at daemon start.

## Settings (any layer)

One table shape for every layer (see "Effective config"): `[settings]` of the orchestrator, a project and a rig;
the `[agent]` contribution of a plugin; the top level of `agent.toml`; an agent variant `[agents.<name>]`; and the
overrides `[overrides.<agent>]`. A pack's `[settings]` and a prompt step take only the three launch settings:
`harness`, `model`, `effort`. There is no `provider` key: a layer that has one is an unknown field.

| Key | Type | Merge |
|---|---|---|
| `harness` | string: the agent CLI, one of the orchestrator's `[harness.*]`; see "Harness and model" | scalar |
| `model`, `effort`, `system_prompt`, `permission_mode` | string; `permission_mode` is the harness's own: claude's `--permission-mode`, codex's approval policy (`untrusted`, `on-request`, `never`) | scalar |
| `inherit_host` | boolean | scalar |
| `env` | table of strings | by key |
| `mcp.<name>` | `{command = [..], env, scope}` (stdio; `scope = "orchestrator" \| "project" \| "session"`, optional) or `{url, headers}` (HTTP), not both (a server the daemon runs is a `[[service]]`, below); a key of the other kind (`headers` with `command`, `env` or `scope` with `url`) is an error | by key, entry replaced whole |
| `hooks.<name>` | `{event, matcher, command, timeout}` (harness hooks, Claude dialect) or `{enabled = false}`; `event` must be a hook event of claude 2.1 or one codex adds (`Interrupt`, `PreModelRequest`, `PostModelRequest`) | named list |
| `skills.<name>` | `{path}` (must exist) or `{enabled = false}` | named list |
| `plugins` | array of `{name, enabled = true}`, in attachment order; parameters belong to the `[[plugin]]` declaration | named list |
| `native.<harness>` | any table, passed to the adapter as is; `native.claude` is a table of claude settings keys, delivered with `--settings` together with the hooks; `native.codex` is a table of codex `config.toml` keys, delivered to the app-server as `-c key=value` flags (so it is part of the process fingerprint) | by key |

### MCP servers and services (spec §11.4)

- `url` alone: an HTTP server someone else runs. Every session gets it with its own `headers` and the identity
  headers `X-Aae-Project`, `X-Aae-Rig`, `X-Aae-Agent` and `X-Aae-Run`.
- `command` alone: a stdio server. With `scope = "session"` the harness starts it in each session. Otherwise
  (`orchestrator`, the default, or `project`) it is shared: the daemon runs it as a service behind the bridge
  `aae mcp-bridge -- <command>` (internal subcommand of the `aae` binary, `crates/aae/src/bridge.rs`; the bridge is
  not part of the core), whose MCP endpoint `/mcp` the sessions get as for a service with `mcp`. The bridge
  initializes the server once, answers every client's handshake itself in both protocol epochs, passes each request
  under its own id with the identity headers as `params._meta` (`aae/project`, `aae/rig`, `aae/agent`, `aae/run`),
  and does not pass the server's own requests and notifications to clients. A server with state per client (a
  browser) or that needs the session's working directory (the bridge starts in `/`) needs `scope = "session"`.
- `command` and `url` together: an error; a server the daemon runs is a `[[service]]` with `mcp` (below).
- One server per scope, not per session, is the default: one HTTP MCP server served claude, codex and pi, and the
  identity reached it in the headers (2026-11-10; claude 2.1.321, codex 0.166.0, pi 1.5.0).
  <!-- MEASURE id=shared-http-mcp date=2026-11-10 ver=claude-2.1.321,codex-0.166.0,pi-1.5.0 env=linux-arm64-orbstack src=reports/36 note="6 calls, 2 per harness; not checked under load" -->
- The server aae serves, and any shared server recommended with it, speaks both MCP protocol epochs: stateless
  2026-07-28 and the `initialize` epoch ([mcp.md](../api/mcp.md#protocol-epochs)). codex 0.166 speaks only
  2025-06-18 and refuses a server of the new epoch only.
- A restart of a shared server has to be quick: claude reconnects to a restarted stateful server in 7–26 s, and in
  one run of four the turn failed (2026-11-10, claude 2.1.321); a stateless 2026-07-28 server restarts unnoticed.
  <!-- MEASURE id=claude-mcp-reconnect date=2026-11-10 ver=claude-2.1.321 env=linux-arm64-orbstack src=reports/36 note="4 runs: 6.8 / 15.7 / 20.6 / 25.7 s; stateful HTTP MCP, restarted on the same port; on stateless 2026-07-28 the restart is transparent" -->
  A failed MCP server is a failed tool, not a failed run.

A **service** is a `[[service]]` table at the top level of the orchestrator config, a project's `aae.toml` or a
`plugin.toml` (not of `[settings]`): a long-lived process the daemon supervises, with MCP optional.

```toml
[[service]]
name = "mail"                                  # required; no `.`; unique in its file
command = ["mail-d", "--port", "${AAE_SERVICE_PORT}"]  # required, non-empty
env = { MAIL_DIR = "/srv/mail" }               # optional, added to its clean environment
scope = "orchestrator"                         # | "project"; `session` is an error
health = { http = "/health" }                  # | "http" (= /health) | { command = [...] }; none: the port accepts
mcp = { url = "/mcp" }                         # | true (= /mcp); none: no MCP endpoint
```

- Layers: services merge by name like `mcp` entries; a stronger layer replaces a service whole. An orchestrator or
  project service is used by every session of its level; a plugin's by the sessions of the agents that attach the
  plugin. `${AAE_PLUGIN_DIR}` in a plugin's service `command`, `env` and `health.command` is the plugin directory.
  `explain` prints `service.<name>` with its layer.
- The daemon runs `command` once per scope (`orchestrator`, the default, or `project`), lazily, when the first
  session that uses it starts, and stops it 5 minutes after its last session ended. The process starts in `/` with a
  clean environment with `env`, `AAE_SERVICE_PORT` (a free port on 127.0.0.1 picked by the daemon; also replaces
  `${AAE_SERVICE_PORT}` in `command`) and `AAE_SERVICE_TOKEN`; it must refuse requests without
  `Authorization: Bearer <token>`.
- With `mcp`, sessions get `http://127.0.0.1:<port><path>` as an MCP server of the service's name (it replaces an
  `mcp` entry of that name) with the token and the identity headers. Without `mcp`, sessions get
  `AAE_SERVICE_<NAME>_URL` (`http://127.0.0.1:<port>`) and `AAE_SERVICE_<NAME>_TOKEN` in their environment
  (`<NAME>`: the name in upper case, other characters than letters and digits as `_`). Either way a session start
  waits up to 30 s for the service to be `ready` and gets the port it is ready on.
- Health decides readiness only: the service is `ready` once its check passes (checked every 100 ms, each check
  up to 5 s; `http` is a `GET` with the token answering 2xx; `command` runs with the service's environment and
  passes on exit 0). Not ready within 30 s, it is stopped and fails. Once ready, a live service is one whose
  process lives: there is no periodic check. A server that is not up is a tool failure, not a failed run.
- The port is checked free right before each start; one taken (someone else listens there and would get the token) fails
  that start without running `command`. A port or `http` check passes only while the service's processes (the ones with
  its `AAE_INSTANCE` marker) alone listen on the port where a connection to `127.0.0.1` reaches (`127.0.0.1`, `0.0.0.0`,
  `::`, `::ffff:127.0.0.1`), so a process that takes the port while the service starts gets no check and no token; the
  service then fails to bind and starts again. So a service whose listener is not its own process (`docker run -p`: the
  Docker daemon's proxy listens; a wrapper that clears the environment, as `env -i` or `sudo` without `-E`) never passes
  a port or `http` check: give it `health = { command = [...] }`. A `command` check checks what it likes, and does not
  tell who listens: a process that takes the port while such a service starts may get the token. A service that dies or
  fails is started again (with the effect backoff) while sessions use it: on a new port until it was first `ready`, then
  on the same port, so the sessions holding its URL reconnect, unless two starts in a row find someone else listening
  there: then on a new port with a new token, which new sessions get; the sessions holding the old URL lose the tool,
  and whoever holds the old port cannot use the old token. Every move to a new port comes with a new token. With no
  sessions it is dropped. A session start that waited 30 s in vain gets port 0 and no token: the port the service was
  not ready on may be someone else's. On macOS the listener is not told (`/proc` only), any listener passes the check. A
  changed `command`, `env` or `health` is a new service; the old one idles out. Events: `service.starting`,
  `service.ready`, `service.failed`, `service.stopped`; `aae instance list` lists services too.
- `url` with `${AAE_API}`: the daemon's own API on TCP, e.g. `url = "${AAE_API}/mcp"` for its MCP frontend. The
  placeholder becomes `http://<api_tcp>` and the session gets `Authorization: Bearer <token>` with the token of the
  daemon's start. With no `api_tcp` the session goes without the server, as with a service that is not up.

## Effective config and "explain config"

Settings live on three levels: the orchestrator (`~/.config/aae/config.toml`: the harnesses with the default model of
each, shared MCP, hooks, skills, env, plugins), the project (`aae.toml`: rigs, packs, project plugins, overrides) and
the rig (a section of `aae.toml`: repository path, variables, overrides). An agent is not a level but a consumer: its
`agent.toml` gives the defaults of the role, and every setting, the harness and the model too, comes from the
layers, weakest first:

1. the harness's own host config (`~/.claude`, `~/.codex`), when `inherit_host` is on;
2. the orchestrator's `[settings]`, over the default model of the harness;
3. the project's `[settings]`;
4. the rig's `[settings]`;
5. the `[agent]` contributions of the plugins attached to the agent, in attachment order;
6. the agent's own `agent.toml`: the defaults of the role;
7. `[settings]` of the pack of the run's formula (launch settings only);
8. the `[[step]]` of the formula the run is at (launch settings only);
9. the agent variant the rig runs in place of the agent (`[agents.<name>]`, see "Agent variants");
10. `[overrides.<agent>]` of the orchestrator, then the project, then the rig: they fix an agent of an outside pack
   without forking the pack. A variant takes the overrides named after it, not those of its base agent.

Layers 7 and 8 belong to a run: the agent alone (`explain` without a formula, `aae doctor`) has none of them. What
the user says for the agent (9, 10) is stronger than what the pack and the formula say.

A scalar takes the strongest layer's value; a table (`env`, `mcp`, `native`) merges by key, an entry replaced whole;
a named list (`hooks`, `skills`, `plugins`) is the union, and a stronger layer turns an element off by name. The host
layer is the exception: the harness merges it itself, aae neither reads nor parses it, and it is turned off only as a
whole (`inherit_host = false`), never one element by name. The harness config inside the rig (`CLAUDE.md`,
`.claude/`, `.mcp.json`, `AGENTS.md`) belongs to the repository, not the host: it applies with inheritance off too
(see "Host inheritance").

`Config::effective(project, rig, agent)` merges the layers above for one agent alone: the harness default model,
orchestrator, project and rig settings, then the contributions of the attached plugins in attachment order, the agent,
and the overrides of the orchestrator, the project and the rig. `Config::for_run(project, rig, formula, step)` is what a
step of a run gets: the agent the rig runs for that step (a variant by its name), and its config with the pack, step and
variant layers too; a run keeps the one of its current step. Plugins attach first (the `plugins` key of every layer), so
a stronger layer can detach a plugin and its whole contribution. An override applies when its key resolves (see Names)
to the agent. Every value keeps its layer; a value several layers make together (`native` tables merged by key, a system
prompt with plugin fragments, `PATH` with plugin `bin/` directories) names them all in order (`# agent + plugin mail`),
and a model the adapter expanded keeps the short name it was given (`model = "gpt-6-luna"  # rig override, given as
"luna"`). The new layers print as `pack <name>`, `formula <pack>.<name> step <step>` and `variant <name>`. `explain()`
prints one line per value with its layer and one line for the host: inherited by the harness and invisible to aae, or
off (`aae config explain --project <p> --agent <a> [--rig <r>]`, with no daemon, on the file on disk; `GET
/v1/config/explain`, on the config the daemon holds). The pack, formula and variant layers count only for a run, so they
show with `--formula <f> --rig <r>` in place of `--agent`, step by step: for a prompt step a header ``# step `<s>` of
`<f>` on `<r>`: agent <a>`` (the rig's variant if it maps one) and its effective config, for an exec step one line ``#
step `<s>` of `<f>`: exec `<command>` ``. The API answer then has `agent` null and `steps: [{step, kind, agent}]`. The
effective config serializes to JSON: a run keeps it as its snapshot, and `env` is the whole session environment besides
what the adapter adds: a session does not inherit the daemon's environment wholesale. The core assembles it when the run
prepares, right before the session starts; the adapter materializes it into launch flags, session parameters or a
generated harness config directory, which lives in the state directory, never in the rig or the harness's home
directory. The adapter's `split` turns it into a process part, keyed by its fingerprint (contracts 8), and a session
part.

## Harness and model

`harness` is the agent CLI that runs the session (`claude`, `codex`); the user names it, there is no `provider`
setting (spec §6.2). Which vendor serves the model is the harness's own setting (its host config or
`native.<harness>`, e.g. codex's `model_provider`); the core does not read it. `harness` changes by the same layers
as the other launch settings: an override, a variant, a pack, a formula step.

The harness must be one of the orchestrator's `[harness.*]`, and some layer must set it. These are load errors:
every prompt formula on every rig of a project must assemble, and an error names the project file, the formula and
rig (`formula `kit.implement` on rig `backend`: no layer sets `harness``).

`model` is a full name, passed to the harness as it is, or a short name of the adapter's table, expanded to the
latest model of that family when the config is assembled: codex takes `astra`, `luna`, `terra`, `sol`; claude
resolves `fable`, `opus`, `sonnet`, `haiku` itself, so they go to it as they are. The model gate of a run is
`model:<harness>/<model>` with the expanded name (contracts 7), known from the config before the first attempt.

## Host inheritance (spec §6.4)

`inherit_host` is on by default and is a scalar setting of any layer. On, the harness reads the operator's own
config (`~/.claude`, `~/.codex`) as it would outside `aae`; `aae` does not see that layer and shows it as one line
in "explain config". Off, the session gets its own config directory in the state directory and only what `aae`
assembled, plus the rig's own files (see "Consequences" in `docs/design/experiments.md`).

What inheritance costs, and what the user should know:

- the operator's personal config adds 1.6–2.0 s to a claude start, almost all of it connecting the operator's MCP
  servers;
  <!-- MEASURE id=claude-host-config-start date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="haiku, n=3-7; +1.97 s until init; version taken from CONTEXT.md, report 42 does not record it; re-measure on the owner's current config" -->
- the operator's stdio MCP servers start in every session, about 45 MB per session on the owner's setup. Shared
  MCP (one process for many sessions) covers only the servers declared in the `aae` config; `aae doctor` warns
  when it sees stdio MCP servers in the host settings (`crates/aae/src/doctor.rs`);
  <!-- MEASURE id=stdio-mcp-per-session date=2026-11-10 ver=prior-a86d05bd7 env=linux-arm64-orbstack src=reports/02,reports/18 note="deja server, PSS 42-62 MB per session; deja version not recorded; depends on the operator's MCP set" -->
- the operator's own hooks and plugins run in agent sessions;
- codex rewrites `~/.codex/config.toml` itself (hook trust hashes); the adapters add no writes of their own to
  the host's files.

An inheriting process gets the daemon's proxy and certificate variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`,
`NO_PROXY` in either case, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `NODE_EXTRA_CA_CERTS`) and, for claude,
`CLAUDE_CONFIG_DIR`; an isolated one gets none of them (set them in `env` if it needs them). A codex agent with
skills that inherits the host gets a `CODEX_HOME` of its own in the state directory (codex reads skills only from
`CODEX_HOME/skills`): it links the host's `auth.json`, `config.toml`, `AGENTS.md` and every skill of
`~/.codex/skills` beside the agent's own (an agent's skill of the same name wins); nothing else in `~/.codex` reaches
such an agent, and `HOME` stays the operator's.

codex runs with `sandbox_mode = "danger-full-access"` unless the native block, or the host's `config.toml` of an
inheriting process, sets `sandbox_mode` (spec §2, decision 38): the codex sandbox needs unprivileged user namespaces,
which many hosts forbid (`docs/design/experiments.md`). Commands are then not confined to the workdir; `aae doctor`
names each such agent, as `<project>/<rig>@<pack>.<agent>`, in a warning; an agent whose config does not assemble
cannot be shown to set it and is named too.

With inheritance off, credentials still come from the host; a session whose `env` carries a credential of its
own uses that one instead.

- claude (`crates/aae-claude/src/part.rs`): an own `CLAUDE_CONFIG_DIR` and `--setting-sources project,local`;
  the subscription access token is read from the host's `.credentials.json` at every spawn and passed as
  `CLAUDE_CODE_OAUTH_TOKEN`, so a token the host refreshed reaches the next process. Own credentials:
  `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`.
- codex (`crates/aae-codex/src/part.rs`): an own `CODEX_HOME` and `HOME`; the host's `auth.json` is linked into
  that `CODEX_HOME`, and the model provider the host config selects comes as the first `-c` flags, its `env_key`
  variable passed from the daemon's environment (`host_provider`). The same variable is passed when inheriting.
  Own credentials: `OPENAI_API_KEY`, `CODEX_API_KEY`.

Known limits:

- not verified: what happens when an isolated claude's subscription token expires mid-session; a ChatGPT
  subscription login in a separate `CODEX_HOME` (the owner's host logs codex in by a provider key);
- macOS keeps the claude login in the keychain, which is not read (the file only);
- of a host provider, only `env_key` is passed from the environment (`env_http_headers` is not); an isolated codex
  gets the provider without `http_headers` and `experimental_bearer_token`, so a provider whose login is only there
  is not logged in under isolation (and `doctor` reports such a provider's login as not checked).

Checked by criterion 12 of the acceptance spec (`crates/aae-acceptance`), the unit tests of both adapters and
`crates/aae/tests/doctor.rs`.

## Orchestrator: `~/.config/aae/config.toml`

```toml
projects = ["~/work/shop"]          # project directories, each with an aae.toml
max_runs = 8                        # runs started at once across all projects (default 8, at least 1)
bead_failures = 3                   # failed runs of a bead before the core closes its gate (default 3, at least 1)
api_tcp = "127.0.0.1:7171"          # the API on TCP too (off by default; loopback only; a reload cannot change it)
timezone = "Europe/Berlin"          # IANA zone `cron` is read in (default: the system zone; an unknown name is an error)

[harness.claude]                    # the harnesses of this host
model = "sonnet"                    # default model of the harness
binary = "/opt/claude/bin/claude"   # optional explicit path; otherwise mise, then `PATH`

[harness.codex]                     # keys beyond `model` and `binary` are the adapter's; claude takes none
max_sessions = 8                    # codex: threads per app-server before a second one starts (default 8)
instance_idle_s = 60                # codex: seconds an app-server without threads lives on (default 60; 0 stops it at once)

[[plugin]]                          # an installed plugin; its orchestrator side is enabled
path = "~/aae-plugins/aae-plugin-mail"

[builtin.default-policy]            # a bundled plugin (see below); on with no table
backoff_cap_s = 300

[watchdog]                          # thresholds of invariant 15 (contracts 6.1); every key optional
silence = "5m"                      # no event after the ack, the model thinking, after a turn ended
tool_silence = "10m"                # a tool running
background_silence = "30m"          # background work after the turn
human_timeout = "1h"                # a request waiting for its answer
start_timeout = "120s"              # the bead claim, then the ack of the first prompt
queue_timeout = "168h"              # a run not started
park_timeout = "168h"               # a parked run on a gate without `until`

[settings]                          # layer 2
[overrides.implementer]             # layer 10, for an agent of any project
```

Each `[watchdog]` value is a duration with one unit (`ms`, `s`, `m`, `h`) from 1 ms to 8760 h; an unknown key is an
error. A reload applies to the deadlines set from then on: a deadline already set keeps its time.

## Project: `<project>/aae.toml`

```toml
name = "shop"                       # optional, defaults to the directory name; unique among projects
packs = [
  "../packs/kit",                    # a local pack directory
  { git = "https://github.com/acme/aae-pack-review", rev = "v1.2.0" },  # a pack from git: branch, tag or commit
]

[[plugin]]
path = "plugins/aae-plugin-notes"

[settings]                          # layer 3
[overrides.implementer]             # layer 10

[agents.codex-implementer]          # a variant of a pack agent (see "Agent variants")
base = "kit.implementer"
harness = "codex"
model = "luna"

[rigs.backend]
path = "../backend"                 # must exist
vars = { test_cmd = "cargo test" }   # `{{ rig.test_cmd }}` in a prompt; `name` and `path` are taken by the rig
triggers = ["nightly-audit"]        # pack triggers that run on this rig (a pack does not know rig names, §9.5)
agents = { implementer = "codex-implementer" }  # on this rig the variant runs where a formula names `implementer`
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

### Packs and plugins from git: `aae.lock` and `aae fetch`

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
says to run `aae fetch`. A checkout whose `.git/HEAD` is not its pinned commit (someone checked out another
revision in it) is an error too: remove the directory and run `aae fetch`. Edits to the files of a checkout are not
detected. Commit `aae.lock` with the project; `.aae/` is a cache to ignore.

A plugin runs any command with the user's rights, so nothing installs or enables one implicitly: its orchestrator
side runs only once a `[[plugin]]` declares it, its agent side only for the agents that attach it in `plugins`, and a
plugin from git runs only at the commit pinned in `aae.lock`.

## Pack: `<pack>/pack.toml`

```toml
name = "kit"
version = "0.3.0"
requires_plugins = ["mail"]         # must be declared by the orchestrator or the project
min_aae = "0.1.0"                   # optional

[settings]                          # layer 7: launch settings of the runs of its formulas
model = "sonnet"

[[trigger]]
name = "implement-next"
on = "bead.ready"
where = { "bead.labels" = "auto" }
run = "implement"
inputs = { bead = { next_ready = true, epic = "{{ event.bead.epic }}" } }
```

Content: `agents/<agent>/agent.toml` (plus `prompt.md` and any other files of the agent, see "Agent") and
`formulas/**/*.toml`: every `.toml` file under `formulas/`, in subdirectories too, is a formula. A pack
is the unit of content: agents, formulas and triggers. It may require plugins but never contains one; a plugin is
declared by the orchestrator or the project.

## Names

- Pack, agent, formula, trigger, rig and project names are non-empty and have no `.`; agent, formula and trigger
  names are unique within a pack, pack names within a project.
- A reference is `<name>` or `<pack>.<name>`. A bare name resolves to the referring pack's own item first, then to
  the only pack that has it; a name that two other packs share must be qualified.

## Agent: `agents/<agent>/agent.toml`

`prompt` (role prompt file, default `prompt.md`
when it exists; its text is the agent layer's `system_prompt`, so an override may still replace it, and setting
`system_prompt` in `agent.toml` too is an error), `max_runs` (optional positive integer: runs of this agent
started at once in its project; the orchestrator's `max_runs` still applies), then the settings keys above. Its
`harness`, `model` and `effort` are the defaults of the role: a pack, a formula, a variant or an override may change
any of them, and an agent that names no `harness` needs a layer that does.
A role prompt file that is empty or only whitespace is a load error naming the file.
Everything of one agent (settings, prompts,
skills, hooks, scripts) lives in its directory, laid out as its author likes. The core has no role names: whatever a
role does is configuration.

A run counts toward both limits from `preparing` until it is terminal, except while `parked` (no harness process).
A run over a limit waits in `queued` with `waiting_for = "concurrency"`; a busy workdir is reported first.

## Agent variants

`[agents.<name>]` of the orchestrator or a project declares a pack agent under another name with settings of its own:
`base = "<pack>.<agent>"` and any keys of `agent.toml` but `prompt` (the role prompt, skills and directory are the
base agent's). A rig runs a variant in place of an agent the formulas name: `[rigs.<r>] agents = { <agent> =
"<variant>" }`. A run of the variant is of the variant's name: `max_runs` (its own, else the base agent's), the gate
`agent:<variant>`, the events and `explain` count it as an agent of its own. The formulas of the pack still name the
base agent; the other way to the same change, without a new name, is an override (`[rigs.<r>.overrides.<agent>]`).

A project's variant replaces an orchestrator's of the same name. Load errors: a `base` that resolves to no agent; a
`base` that names a variant (a variant is of a pack agent, one level); a rig that maps to a variant nobody declares; a
variant named like an agent of a pack of the project.

## Formula: `formulas/<file>.toml`

```toml
name = "implement"
description = "Implement the task of a bead"
worktree = true                     # optional (default false): each run works in a git worktree of its own

[inputs]
bead = { type = "bead", required = true }   # bead | string | integer | boolean

[[step]]
name = "implement"                  # optional (default: the position, "1", "2", ..)
kind = "prompt"                     # prompt: agent + prompt template; exec: command
agent = "implementer"
prompt = "implement.md"             # relative to the formula file
timeout = "45m"                     # optional
model = "opus"                      # optional, prompt steps only: layer 8, the launch settings `harness`,
                                    # `model`, `effort`

[[step]]
name = "test"
kind = "exec"
command = ["cargo", "test"]
```

A formula has a name, a description, declared inputs and one or more steps, which a run goes through in order
(contracts 6.1): the next step starts when the one before passed, the first failed step fails the run, and the run's
outcome is that of its last step. Step names are unique in the formula and contain no `.`. A `prompt` step is a turn of
the agent in a harness session, and its outcome comes from the finisher (`aae finish`, contracts 6.3). An `exec` step is
a command with no model that the daemon runs in the run's workdir, in a process group of its own, under the step
timeout; exit 0 is `pass`, 75 (`EX_TEMPFAIL`) a failed attempt (not an outcome), any other `fail`, and a bounded tail of
its output is kept in the result (contracts 6.2). An `exec` step has `command = ["prog", "arg", ..]` (non-empty) and
`timeout` instead of `agent` and `prompt`. Nothing is substituted into the command; it gets the run in its environment:
`AAE_RUN`, `AAE_BEAD` (when the run has a bead) and `AAE_INPUT_<NAME>` per input (the name upper-cased, other characters
than letters and digits as `_`; a string as it is, any other value as JSON). Two inputs that give one variable
(`note-text` and `note_text`) are a config error.

A step sees the outcomes of the steps before it, by name (the last attempt of each): a prompt template as
`{{ steps.<name>.result }}`, `.summary` and `.data.<key>` (contracts 2.3; `data` of an exec step is `{exit_code,
stdout, stderr}`), an exec step as `AAE_STEP_<NAME>_RESULT` (`pass` or `fail`), `AAE_STEP_<NAME>_SUMMARY` and
`AAE_STEP_<NAME>_DATA` (JSON, when there is data), `<NAME>` formed as for inputs; a step name whose variables collide
with another step's or an input's is a config error. Each prompt step has a session of its own; a run that continues a
session (`continue_from`) continues it in its first step only, so a formula whose first step is `exec` cannot be
continued.

A run start checks the inputs: no undeclared one, every `required` one present (`null` is no value), an `integer` input
a JSON integer or its text, a `boolean` one a JSON boolean or `true`/`false` as text, a `string` one a string (the CLI
sends `--input` values as text). A `bead` input is a bead id, `{ next_ready = true, epic = <id> }` or `{ request = "<text>" }`: a
request (only that key, a text that is not blank; the rig must be named) has the tracker create the run's root bead as
the run prepares, titled with the first line of the text and described by all of it, and the input then holds its id
([tracker-br.md](tracker-br.md#beads-and-runs)); a bead named by id
that carries a `rig:` label of another rig is refused, at the start when the ready set knows the bead, else at its claim
(the run fails with `bead_of_another_rig` and the claim is released). `worktree = true` gives each run of the formula,
of either step kind, a git worktree that aae makes and removes (the bundled `worktree`, see below).

A prompt template is checked at load (contracts 2.3): `inputs.<name>` must be a declared input; `bead.<field>` needs
a `bead` input and a field of `Bead` (contracts 5.2); `run.<field>` a field of `RunView` (1.3); `project.name`,
`project.path`; `rig.name`, `rig.path` and `rig.<var>` (a rig variable, checked when the run starts).
A prompt is never empty: a template file of only whitespace is a load error, and a prompt that is empty after
substitution fails the run (`infrastructure`, `prompt: empty after substitution`) before any process starts.

## Trigger

`name`, `run` (a formula reference) and exactly one source: `on` (an event type of contracts 1.3, with optional
`where`), `cron` (five fields) or `every` (a duration). `inputs` follows contracts 2.3: every key is an input of the
formula, every required input is given, placeholders exist only with `on` and their paths exist in that event's
payload, as do the paths of `where`. `rig` is allowed only in a project's `[[trigger]]`.

- A trigger runs on the rigs that list it in `triggers` and, for a project trigger, on its `rig`; one that no rig
  names never fires. An event trigger sees the events whose `project` and `rig` are its own or null; a `where` on
  `rig` names the rigs it listens to instead, so a chain can cross the rigs of a project
  (`where = { rig = "backend", "run.formula" = "kit.build" }` on the rig `frontend`).
- `cron` is read in the orchestrator's `timezone`, the system zone by default: minute, hour, day of month, month, day
  of week; each field is `*`, a value, `a-b`, any of them with `/step`, or a comma list; months and days also take
  `jan`..`dec` and `sun`..`sat`, and day 7 is Sunday. When both day fields are restricted, a day matching either fires
  (as in Vixie cron). A local time skipped by a clock change fires when the clock moves on (02:30 in a spring gap
  fires at 03:30); a local time that repeats fires once, on its first pass, so `*/15` is silent in the repeated hour's
  second pass.
- `every` first fires one interval after the daemon first sees the trigger, then one interval after each firing.
  A schedule missed while the daemon was down fires once at start, and the next one is counted from then.
- A trigger on `trigger.skipped` is a config error: a skip that fires a trigger could skip again forever.
- A run started by a trigger is one level deeper than the run of its event (1 without one, a schedule included);
  past depth 10 the firing is skipped with `depth`. Loops are bounded by this depth, not by the length of a chain:
  a long epic is legal. A `bead.ready` event does not add a level: a bead made ready by the run that closed its
  blocker is the tracker's next work, not a run that run started, so its run keeps the depth of the event's run
  (at least 1) and an epic of beads each blocked by the last runs to its end. Repeated runs over one bead are
  bounded by the bead's failure count (`bead_failures`).

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

[agent]                             # layer 5 contribution to the agents that attach the plugin
skills.mail = { path = "skills/mail" }

[[hook]]                            # orchestrator hook (§11.3, contracts 3)
on = "run.finishing"
command = ["bin/check-mail"]
timeout = "10s"                     # optional
```

Plugin names are unique across the orchestrator and the project; an attachment in `plugins` must name a declared plugin,
and the `[agent]` of a plugin has no `plugins` (a plugin attaches no plugins). A declaration (a `path`, or a `git`
source as for packs above) may carry parameters, `[[plugin]] path = ".." config = { .. }`: its orchestrator hooks get
them in `AAE_PLUGIN_PARAMS` (contracts 3.1). In the `[agent]` contribution, `${AAE_PLUGIN_DIR}` in a harness hook
`command`, an `env` value, an MCP `command` or `env` value and any string of a `native` table is replaced with the
plugin's directory at load, so they reach the plugin's own files; skill paths are relative to it. A Claude Code plugin
directory (`--plugin-dir`) cannot be handed over: the adapter takes that flag for skills, so a plugin gives its skills,
hooks and MCP servers through `[agent]`. Beyond its `[agent]` layer a plugin gives its attached agents two things, in
attachment order: the text of its `prompt` file, after the system prompt the layers give (each fragment after a blank
line); and its `bin/` directory, when it has one, ahead of `PATH` (the `PATH` a layer sets, or else the daemon's). The
daemon's `PATH` taken this way becomes part of the effective config: it is in the run's snapshot and in the instance
fingerprint, so after a daemon restart with another `PATH` these agents get new instances. The example
`examples/aae-plugin-review` shows both sides.

A plugin is the unit of extension: a typical extension (a mail integration, a check, a reaction policy) is a
plugin, not core code. Its directory holds `plugin.toml` (name, version, parameters, the contribution to agents,
orchestrator hooks, services) and what its parts need (`skills/`, `hooks/`, `bin/`, ...). Its agent side reaches
only the agents that attach it, native parts of a harness included (`native.<harness>`); a contribution that changes
the process part of a harness config gives those agents instances of their own, by the fingerprint (contracts 8).
Its orchestrator hooks keep no state: an event carries the attempt counters, times and gate states a policy needs,
and the core has no storage for plugins.

## Bundled plugins: `[builtin.<name>]`

The plugins bundled with aae (spec §11.5) run in process as hooks of the contract of contracts 3, after every
declared plugin, so a declared plugin decides first. `enabled` (boolean) turns one on or off; every other key is a
parameter, checked when the daemon starts. A table with parameters and no `enabled` turns the plugin on. A
project's `aae.toml` may have `[builtin.<name>]` tables too: for the runs of that project such a table replaces the
orchestrator's table of that plugin whole (spec §11.3: orchestrator-side plugins are turned on for the orchestrator
or for a project).

- `default-policy` (on unless `enabled = false`) decides on `run.attempt_failed` and `run.turn_ended_without_outcome` as
  in contracts 10. Parameters, all integers: `quota_margin_s` (60), `quota_wait_s` (1800; the wait of `quota_exhausted`
  when neither `retry_at` nor the account's `resets_at` is known; an account is used up when `rate_limit.utilization >=
  1.0` and `resets_at` is in the future), `model_pause_s` (600), `backoff_base_s` (10), `backoff_cap_s` (300),
  `backoff_park_from` (3), `auth_backoff_base_s` (30), `auth_backoff_cap_s` (3600), `rapid_s` (30), `crash_loop` (5),
  `crash_pause_s` (300), `resume_failures` (3), `unknown_after_s` (30), `unknown_retries` (3). The waits and caps that a
  zero would turn into a hot loop (`quota_wait_s`, `model_pause_s`, both bases and both caps, `crash_pause_s`) and
  `crash_loop` must be at least 1; `quota_margin_s`, `rapid_s` and `unknown_after_s` must not be negative.
- `worktree` has no table: it is on for the runs of the formulas with `worktree = true` (a formula setting, spec §2,
  decision 48), and a `[builtin.worktree]` table is a config error saying so. Each such run gets a git worktree of its
  rig on the branch `aae/<run>`, in `<git common dir>/aae/worktrees/<run>`, as its workdir. When the run ends the
  worktree is removed unless it has uncommitted changes or a HEAD no ref reaches (then `plugin.hook_failed` says why);
  the branch is deleted only when it is merged. A run with `continue_from` starts in the workdir of the run it continues
  (a session resumes where it ran); when that is a worktree of this plugin, it is brought back on its branch if it was
  removed, whatever the run's formula says. A reload that leaves a project no formula with `worktree = true` turns the
  plugin off there: the worktrees of its runs still going are left in place, as are their branches. Its hook has the 30
  s timeout of a hook; a `git` still running at that timeout is killed with its process group and the hook fails.

### Where the bundled plugins end and the core begins

The mechanisms live in the core: resuming an attempt, a new session that keeps ownership, issue gates, the counters
the policy reads, the ceiling of 20 failed attempts per run (waits on an account gate not counted), the watchdog and
every deadline. The bundled plugins only choose a reaction and its thresholds; they never change a harness. With
`default-policy` off, the "no plugin decision" column of [contracts 1.3](contracts.md#13-payloads) applies: a failed
attempt ends the run `failed` with class `infrastructure`. The bundled plugins are compiled into the same binary, need
no external runtime, and are counted on their own budget line (`plugins` in `scripts/budget.sh`, 1500 lines) inside the
release total.

Postmortem 0001 (`docs/postmortems/0001-loop-stalled-after-transient-auth-error.md`) and the owner's decision of
2026-11-14 (spec §2, decisions 49, 50) shaped `auth_required` and `billing`: a proxy or WAF 403 passes by itself, so every
such failure parks the account gate for a wait that grows with each failure in a row up to an hour, and the next
attempt is the probe. Nothing goes to an operator and there is no alerting: the journal and the logs tell it. The
core does not count these attempts against its ceiling, so a long outage never ends in `failed{attempts_ceiling}`.
The raw error stays in `failure.raw`. Not done: telling a real logout from a WAF page by the raw error; a real
logout is probed once an hour until someone logs in.

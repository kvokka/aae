# Configuration files

The files the daemon reads, the rules the loader enforces, and how the layers merge into the config of one session
(see "Effective config and "explain config"" below). Durations are strings
with one unit, as in [`contracts.md`](contracts.md) (`"500ms"`, `"30s"`, `"45m"`, `"2h"`). A relative path is
relative to the directory of the file that holds it; a leading `~/` is the home directory. Unknown keys are an
error in every file.

## Loading

- The daemon loads everything at once from the orchestrator file: the projects it lists, their packs, agents,
  formulas, triggers and the plugins they declare. A missing orchestrator file is an empty config. Before reading,
  `aae` downloads the packs and plugins from git into its cache (see [below](#packs-and-plugins-from-git)), as do
  `POST /v1/config/check` and `aae config check|explain` and `aae doctor` with `--config <file>`; the latest
  revision of an unpinned pack is asked for at every load.
- Load is all or nothing: every error found is reported with its file, and an invalid config is never applied.
  The daemon re-reads only on an explicit command: `aae config reload` (`POST /v1/config/reload`); on
  error the previous config stays in force, and `aae config check` reports the errors without applying anything
  (the daemon's file; `--config <file>` loads a file in the CLI, with no daemon).
- A retired key is a config error of one line that says where it is written now: `permission_mode` (the native block of
  the harness), `[overrides.<agent>]` (`[agents.<agent>]`), step kind `prompt` (`ai`), input type `bead` (`task`), the
  `prompt` key of an `ai` step (the agent's `prompt.template.md`).
- A reload is taken by new runs and by the schedules (a schedule whose `cron`/`every` changed, and every `cron` when
  `timezone` changed, is set again from the reload; a removed one is dropped); the snapshot is per step: a running
  step keeps the one it prepared with, and the next step of the run prepares with the config of that moment; an idle
  instance stops with reason `config` only when no agent of the new config, on any rig, starts
  sessions on its fingerprint (a changed MCP restarts only the instances it is part of). It emits `config.reloaded`
  with the generation (1 at daemon start). A change of the set of harnesses, a harness `binary` or any adapter setting
  such as `max_sessions` (a harness `model` is fine), or of which projects have a `.beads` tracker is refused: drivers
  and trackers are made at daemon start.

## Settings (any layer)

One table shape for every layer (see "Effective config"): `[settings]` of the orchestrator, a project and a rig;
the `[agent]` contribution of a plugin; the top level of `agent.toml`; an agent variant `[agents.<name>]`; and the
edit of an agent `[agents.<agent>]`. A pack's `[settings]` takes only the three launch settings: `harness`, `model`,
`effort`; an ai step takes them and `native.<harness>`. There is no `provider` key: a layer that has one is an unknown field. No neutral key
holds a value only one harness understands: the permission mode is claude's `--permission-mode` and codex's approval
policy, with different values, so it is set in the harness's own block (`[settings.native.codex] approval_policy =
"never"`), never as a shared `permission_mode`.

| Key | Type | Merge |
|---|---|---|
| `harness` | string: the agent CLI, `claude` or `codex`, or one named in the orchestrator config; see "Harness and model" | scalar |
| `model`, `effort`, `system_prompt` | string | scalar |
| `inherit_host` | boolean | scalar |
| `env` | table of strings | by key |
| `mcp.<name>` | `{command = [..], env, scope}` (stdio; `scope = "orchestrator" \| "project" \| "session"`, optional) or `{url, headers}` (HTTP), not both (a server the daemon runs is a `[[service]]`, below); a key of the other kind (`headers` with `command`, `env` or `scope` with `url`) is an error | by key, entry replaced whole |
| `hooks.<name>` | `{event, matcher, command, timeout}` (harness hooks, Claude dialect) or `{enabled = false}`; `event` must be a hook event of claude 2.1 or one codex adds (`Interrupt`, `PreModelRequest`, `PostModelRequest`) | named list |
| `skills.<name>` | `{path}` (must exist) or `{enabled = false}` | named list |
| `plugins` | array of `{name, enabled = true}`, in attachment order; parameters belong to the `[[plugin]]` declaration | named list |
| `native.<harness>` | any table, passed to the adapter as is; `native.claude` is a table of claude settings keys, delivered with `--settings` together with the hooks, except `permission_mode`, which goes as `--permission-mode`; `native.codex` is a table of codex `config.toml` keys (`approval_policy` is the thread's approval policy), delivered to the app-server as `-c key=value` flags (so it is part of the process fingerprint) | by key |

A harness hook (`hooks.<name>`) delivered to claude also fires inside every subagent of the session (its input carries
`agent_id` and `agent_type`); it cannot be switched off per subagent. codex reads the same hook config but behaves
differently (by its source at 0.166.0, not exercised here): exit 2 blocks only when stderr is not empty, otherwise
the hook counts as failed; `permissionDecision: "ask"` is rejected and the call goes through; `updatedInput` works
only with `allow`; `prompt` and `agent` handlers are skipped; there is no `StopFailure`, so a hook cannot catch an
API error or a quota. Its matchers name `Bash`, `apply_patch` (aliases `Edit`, `Write`) and `mcp__<server>__<tool>`.

### Permissions

A run is unattended: nothing waits for a human, and every permission is decided by the harness from the config set
before the start, in the `native.<harness>` block of any settings layer (the orchestrator, a project, a rig, an agent
or its variant, a formula's ai step). The defaults are permissive: claude runs with `--permission-mode bypassPermissions`, codex with
`approval_policy = "never"` (beside `sandbox_mode = "danger-full-access"`, "Host inheritance"). What a stricter config
does not allow is denied at once, and the agent sees the denial; each denial the adapter learns of is journaled as
`run.permission_denied` ([contracts](contracts.md#13-payloads)).

- claude: `native.claude.permission_mode` (or `permissions.defaultMode`), the rules `permissions.allow`, `deny` and
  `ask` (an `ask` rule is a denial here: claude runs with `--permission-prompts none`), and the harness hooks
  `PermissionRequest` and `PreToolUse`.
- codex: `native.codex.approval_policy` (its granular form included), `sandbox_mode`, and the `PreToolUse` hook. Under
  a policy other than `never`, a command or file change that needs approval is declined.
- A hook may ask aae or another agent for the decision through the CLI or the MCP of aae: the session environment
  carries `AAE_RUN`, `AAE_ATTEMPT_TOKEN` and `AAE_SOCKET`.

claude refuses `bypassPermissions` when it runs as root (a container, CI): it exits before `init` with
"--dangerously-skip-permissions cannot be used with root/sudo privileges", and the attempt fails. Under root, either
tell claude it runs in a sandbox, `env = { IS_SANDBOX = "1" }`, which keeps the default, or name another mode in
`native.claude.permission_mode` (as `dontAsk` or `acceptEdits`) with the `permissions.allow` rules the agents need.

`aae finish` is a shell call of the agent. The default lets it through; a strict config must allow it explicitly, for
claude `permissions = { allow = ["Bash(aae finish *)"] }`.

### MCP servers and services

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
each, shared MCP, hooks, skills, env, plugins), the project (`aae.toml`: rigs, packs, project plugins, `[agents]` edits) and
the rig (a section of `aae.toml`: repository path, variables, `[agents]` edits). An agent is not a level but a consumer: its
`agent.toml` gives the defaults of the role, and every setting, the harness and the model too, comes from the
layers, weakest first:

1. the harness's own host config (`~/.claude`, `~/.codex`), when `inherit_host` is on;
2. the orchestrator's `[settings]`, over the default model of the harness;
3. the project's `[settings]`;
4. the rig's `[settings]`;
5. the `[agent]` contributions of the plugins attached to the agent, in attachment order;
6. the agent's own `agent.toml`: the defaults of the role;
7. `[settings]` of the pack of the run's formula (launch settings only);
8. the `[[step]]` of the formula the run is at (launch settings and `native.<harness>`);
9. the agent variant the rig runs in place of the agent (`[agents.<name>]`, see "Agent variants");
10. `[agents.<agent>]` without `base` (an edit) of the orchestrator, then the project, then the rig: they fix an agent
    of an outside pack without forking the pack. A variant is edited by an `[agents.<variant>]` table of a stronger
    level, not by the edits of its base agent. An edit may not set `max_runs`; an unknown name without `base` is an error.

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
and the `[agents]` edits of the orchestrator, the project and the rig. `Config::for_run(project, rig, formula, step)` is what a
step of a run gets: the agent the rig runs for that step (a variant by its name), and its config with the pack, step and
variant layers too; a run keeps the one of its current step. Plugins attach first (the `plugins` key of every layer), so
a stronger layer can detach a plugin and its whole contribution. An edit applies when its key resolves (see Names)
to the agent. Every value keeps its layer; a value several layers make together (`native` tables merged by key, a system
prompt with plugin fragments, `PATH` with plugin `bin/` directories) names them all in order (`# agent + plugin mail`),
and a model the adapter expanded keeps the short name it was given
(`model = "gpt-6-luna"  # rig [agents], given as "luna"`). The new layers print as `pack <name>`,
`formula <pack>.<name> step <step>` and `variant <name>`. `explain()` prints one line per value with its layer and one
line for the host: inherited by the harness and invisible to aae, or off
(`aae config explain --project <p> --agent <a> [--rig <r>]` and `GET /v1/config/explain`, on the config the daemon
holds; with `--config <file>`, on that file with no daemon). The pack, formula and variant layers count only for a
run, so they show with `--formula <f> --rig <r>` in place of `--agent`, step by step: for an ai step a header
``# step `<s>` of `<f>` on `<r>`: agent <a>`` (the rig's variant if it maps one) and its effective config, for an exec
step one line ``# step `<s>` of `<f>`: exec `<command>` ``, for a drain step
``# step `<s>` of `<f>`: drain, a run of `<formula>` per bead, <pool> at a time`` (with ``; on a failed bead:
<on_item_failure>`` unless it is `continue`), and after a check
``# a check: on fail, back to `<step>`, at most <n> rounds``, and for a step with `max_attempts` above 1 or a check
script ``# runs at most <n> times; passes when <script> exits 0`` (each part when it applies). The API answer then has `agent` null and
`steps: [{step, kind, agent}]`. The effective config serializes to JSON: a run keeps it as its snapshot, and `env` is
the whole session environment besides what the adapter adds: a session does not inherit the daemon's environment
wholesale. The core assembles it when the run prepares, right before the session starts; the adapter materializes it
into launch flags, session parameters or a generated harness config directory, which lives in the state directory, never
in the rig or the harness's home directory. The adapter's `split` turns it into a process part, keyed by its fingerprint
(contracts 8), and a session part.

## Harness and model

`harness` is the agent CLI that runs the session (`claude`, `codex`); the user names it, there is no `provider`
setting. Which vendor serves the model is the harness's own setting (its host config or
`native.<harness>`, e.g. codex's `model_provider`); the core does not read it. `harness` changes by the same layers
as the other launch settings: an agent edit, a variant, a pack, a formula step.

A harness with an adapter in the build (`claude`, `codex`) needs no `[harness.<name>]` section; the section only sets its
defaults. Any other name is an error: harness `x` is neither of this build nor in the orchestrator config. Some layer
must set the harness. These are load errors:
every ai step of every formula of a project must assemble on every rig and on the root, and an error names the
project file, the formula and rig (`formula `kit.implement` on rig `backend`: no layer sets `harness``).

`model` is a full name, passed to the harness as it is, or a short name of the adapter's table, expanded to the
latest model of that family when the config is assembled: codex takes `astra`, `luna`, `terra`, `sol`; claude
resolves `fable`, `opus`, `sonnet`, `haiku` itself, so they go to it as they are. The model gate of a run is
`model:<harness>/<model>` with the expanded name (contracts 7), known from the config before the first attempt.

## Host inheritance

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
- codex runs every command through the login shell named by `SHELL` (`$SHELL -lc '<command>'`) and snapshots that
  shell's environment on the first turn, so the operator's shell rc files shape every command of the agent;
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
inheriting process, sets `sandbox_mode`: the codex sandbox needs unprivileged user namespaces,
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

Checked by acceptance criterion 12 (`crates/aae-acceptance`), the unit tests of both adapters and
`crates/aae/tests/e2e/doctor.rs`.

## Orchestrator: `~/.config/aae/config.toml`

```toml
projects = ["~/work/shop"]          # project directories, each with an aae.toml
max_runs = 8                        # runs started at once across all projects (default 8, at least 1)
bead_failures = 3                   # failed runs of a bead before the core closes its gate (default 3, at least 1)
api_tcp = "127.0.0.1:7171"          # the API on TCP too (off by default; loopback only; a reload cannot change it)
timezone = "Europe/Berlin"          # IANA zone `cron` is read in (default: the system zone; an unknown name is an error)

[harness.claude]                    # optional: only the defaults of the harness
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
silence = "15m"                     # no event after the ack, the model thinking, after a turn ended
tool_silence = "40m"                # a tool running
background_silence = "120m"         # background work after the turn
start_timeout = "120s"              # the bead claim, then the ack of the first prompt
queue_timeout = "168h"              # a run not started
park_timeout = "168h"               # a parked run on a gate without `until`

[settings]                          # layer 2
[agents.implementer]                # layer 10: an edit of an agent of any project (no `base`)
```

Each `[watchdog]` value is a duration with one unit (`ms`, `s`, `m`, `h`) from 1 ms to 8760 h; an unknown key is an
error. A reload applies to the deadlines set from then on: a deadline already set keeps its time.

The file need not live in `~/.config/aae`: `aae daemon` takes another one with `--config <file>`, and so do
`aae doctor` and `aae config check|explain`, which then check that file with no daemon instead of asking the running
one. A demo keeps its own orchestrator config, `root/config.toml`, next to its project's
`root/aae.toml` for that reason: its `run.sh` starts a daemon of the demo's own with `--config root/config.toml`, so
the demo neither reads nor changes the user's `~/.config/aae/config.toml` ([demos](../../demos/README.md)). The two
files stay separate: `config.toml` lists the projects and the harnesses of the host, `aae.toml` is the project.

## Project: `<project>/aae.toml`

```toml
name = "shop"                       # optional, defaults to the directory name; unique among projects
packs = [
  "../packs/kit",                    # a local pack directory
  { git = "https://github.com/acme/aae-pack-review", rev = "v1.2.0" },  # pinned to a tag or a full commit id
  { git = "https://github.com/acme/aae-pack-tools" },  # no pin: the latest revision of the default branch
]

[[plugin]]
path = "plugins/aae-plugin-notes"

[settings]                          # layer 3
[agents.implementer]                # layer 10: an edit (no `base`)

[agents.codex-implementer]          # a variant of a pack agent (see "Agent variants")
base = "kit.implementer"
harness = "codex"
model = "luna"

[root]                              # optional, as are both keys
name = "root"                       # the default; a rig may not take it
path = "."                          # the default: the directory of aae.toml; relative to it, need not be git

[rigs.backend]
path = "../backend"                 # must exist
prefix = "be"                       # optional: the id prefix of the rig's beads; default, the acronym of the name (`b`)
vars = { test_cmd = "cargo test" }   # `{{ rig.test_cmd }}` in a prompt; `name` and `path` are taken by the rig
triggers = ["nightly-audit"]        # pack triggers that run on this rig (a pack does not know rig names)
agents = { implementer = "codex-implementer" }  # on this rig the variant runs where a formula names `implementer`
[rigs.backend.settings]             # layer 4
[rigs.backend.agents.implementer]    # a table: settings of the agent on this rig (layer 10)

[[trigger]]                         # project triggers may name their rig
name = "audit-backend"
cron = "0 3 * * *"
run = "audit"
rig = "backend"
```

A rig's name is ASCII letters, digits, `-` and `_`. Its prefix is lowercase ASCII letters, digits and `-` inside; it
is the acronym of the words of the name (split on `-` and `_`: `backend` is `b`, `my-project` is `mp`) unless `prefix`
is set. Two rigs of a project with one prefix are a config error. The prefix makes the rig of a bead: its id up to the
last `-` ([tracker-br.md](tracker-br.md#the-rig-of-a-bead)); changing it makes the rig's existing beads beads of the
root.

The project root is itself a pack named after the project: `agents/` and `formulas/` next to `aae.toml` belong to
it, and its triggers are the `[[trigger]]` of `aae.toml`.

The root is where a project works besides its rigs: a run started without a rig works there (unless the
id of a bead an input names has the prefix of a rig), and so does every session of an agent of scope `root`. It is a place like
a rig, under its own name: the run's `rig` in events, its gate `rig:<project>/<root name>`, its workdir, `worktree`
and limits all take it the same way; `{{ rig.name }}` and `{{ rig.path }}` of a run in the root are the root's. The
root has these two settings and no variables: text a project gives an agent is a file of the agent (see "Agent"). Without `[root]` it is `root`, the directory of `aae.toml`; a project of one repository needs no
rigs. When `path` points elsewhere, the config files (`.aae/agents/`, packs) and the tracker stay next to
`aae.toml`. The root need not be a git repository: a run in a plain directory works, and only `worktree = true`
fails that run (`deny`, naming the directory). A rig at the root's path or under the root's name is a config
error. `[root]` takes no `settings`, `agents` or `triggers`: the project's own are the root's, and a project
`[[trigger]]` that names no `rig` fires in the root.

### Packs and plugins from git

A pack in `packs` and a `[[plugin]]` (in the orchestrator file or a project's) is a local directory, or a git
repository with `pack.toml` / `plugin.toml` at its root: `{ git = "<url>", rev = "<tag or commit>" }` for a pack,
`git = ".."` and `rev = ".."` instead of `path` for a plugin (exactly one of `path` and `git`).

- `rev` is a full commit id (40 or 64 lowercase hex digits) or the name of a tag, looked up only as
  `refs/tags/<rev>`: a branch or a short commit id is a config error ("no tag ... in the repository").
- A pack without `rev` follows the latest revision: the remote's `HEAD`, the tip of its default branch, asked for at
  every load of the config. A plugin must be pinned: a `[[plugin]]` from git without `rev` is a config error.

Whoever loads the config downloads first: the daemon at start, at `aae config reload`, `POST /v1/config/check` and
`GET /v1/doctor`, and the CLI at `aae config check|explain` and `aae doctor` with `--config <file>`. The core reads
only the finished directories and never runs git. The cache is one per user, `$XDG_CACHE_HOME/aae/git`
(`~/.cache/aae/git` without it):

- `<cache>/<url>/<commit>/` is a detached checkout of the commit, never changed once in place;
- `<cache>/<url>/tag-<tag>` names the commit a tag pointed at when it was downloaded;
- `<cache>/<url>/head` names the commit of the latest revision last seen.

The url and the tag are written as one file name: letters, digits, `_`, `-` and `.` (not first) as they are, any
other byte as `%XX`. Something not in the cache is cloned in full into a temporary directory next to its place,
checked out detached at its commit and renamed into place. The network:

- a commit whose checkout is in the cache: git is not run at all;
- a tag the cache names, with its checkout there: git is not run either. Tags are taken as immutable: a tag moved on
  the remote is not seen until the cache is cleared;
- no pin: `git ls-remote <url> HEAD` at every load; a commit already in the cache is used as is, any other is cloned.
  When the remote cannot be asked (any failure of `ls-remote`) and the cache names a last seen revision, that
  checkout is used with a warning on the stderr of the process that loaded the config (the daemon's log, the output
  of `aae config check`); with none, it is a config error.

`aae cache clear` runs without a daemon and removes the whole cache; the next load downloads everything again. A
running daemon keeps reading the removed directories until `aae config reload`, so reload right after clearing.

There is no lock file and no command that pins: a pin is only what the config writes in `rev`. Edits to the files
of a checkout in the cache are not detected. git must be in `PATH`; access to the remote (ssh keys, credential
helpers) is git's own, and git never asks for a password (`GIT_TERMINAL_PROMPT=0`). Each git command is killed after
30 s, which fails the load that ran it.

A plugin runs any command with the user's rights, so nothing installs or enables one implicitly: its orchestrator
side runs only once a `[[plugin]]` declares it, its agent side only for the agents that attach it in `plugins`, and a
plugin from git runs only at the tag or commit its declaration pins.

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

Content: `agents/<agent>/agent.toml` (plus `prompt.md`, `prompt.template.md` and any other files of the agent, see
"Agent") and
`formulas/**/*.toml`: every `.toml` file under `formulas/`, in subdirectories too, is a formula. A pack
is the unit of content: agents, formulas and triggers. It may require plugins but never contains one; a plugin is
declared by the orchestrator or the project. A pack holds no word of a user's task: what to do comes only from the
inputs of a run.

Formulas belong to a project. One daemon serves many projects, and a project sees only the formulas of its own packs
(its root included); two projects that use one pack each have their own formulas, and neither affects the other. A
formula is named by its `name` key, not by the subdirectory its file sits in, so the formulas of a project are a flat
list of names.

## Names

- Pack, agent, formula, trigger, rig and project names are non-empty and have no `.`; agent, formula and trigger
  names are unique within a pack, pack names within a project.
- A reference is `<name>` or `<pack>.<name>`. A bare name resolves to the referring pack's own item first, then to
  the only pack that has it; a name that two other packs share must be qualified.

## Agent: `agents/<agent>/agent.toml`

`scope` (`rig`, the default, or `root`: where the agent works. An agent of a rig is an executor bound to one
repository and works in its rig's directory, or in the root on a run without a rig; an agent of the root is bound to
no repository, works over all the rigs of the project and their beads, so planning across several repositories is the
job of a `root` agent, and a `rig` planner plans inside its one repository. An agent of the root works in the
project root whatever the rig of its run, its session there while the run keeps its own workdir, and takes no
rig's layer, so "explain config" of it is the same with a rig or without; `explain` prints `scope = "root"`; a
rig's `agents` that edits it, edits a variant of it or maps it to a variant is a config error, its edits go in the
project's `[agents]`), `prompt` (role prompt file, default `prompt.md` when it exists; its text is the agent layer's
`system_prompt`, so an `[agents.<agent>]` edit may still replace it, and setting `system_prompt` in `agent.toml`
too is an error), `max_runs` (optional positive integer: runs of this agent
started at once in its project; the orchestrator's `max_runs` still applies; a run whose next step is of an agent
at its limit parks until a run of that agent ends or leaves its step), then the settings keys above. Its
`harness`, `model` and `effort` are the defaults of the role: a pack, a formula, a variant or an `[agents]` edit may change
any of them, and an agent that names no `harness` needs a layer that does.
A role prompt file that is empty or only whitespace is a load error naming the file.

`prompt.template.md` is the template of the agent's prompt, in the agent's own directory, so the agent is
self-contained: an `ai` step names the agent and its run gets
this template, rendered (contracts 2.3); the step has no prompt file of its own. A run whose agent a variant or a
bead's `agent:` label gives renders that agent's template (a variant's is its base agent's). An agent that an `ai` step
names and that has no template is a config error naming the path to put it at. The template is checked against every
formula whose steps name the agent, with that formula's inputs and earlier steps.

A project rewrites an agent of its packs with files, without forking the pack: `.aae/agents/<agent>/` next to
`aae.toml` lays over the agent's directory. Its `prompt.template.md` replaces the pack's. `{{ agent.<name> }}` in the
template is the text of `<name>.md`, the project's file over the pack agent's: text the project gives an agent (the
conditions `qa` checks, say `.aae/agents/qa/checks.md`) is a file of that agent, not a string in `aae.toml`. A
`{{ agent.<name> }}` with neither file is a config error naming where to put it; so is a directory under
`.aae/agents/` for an agent no pack of the project has. "Explain config" of a formula names the template file of each
`ai` step.

Everything of one agent (settings, prompts,
skills, hooks, scripts) lives in its directory, laid out as its author likes. The core has no role names: whatever a
role does is configuration.

A run counts toward both limits from `preparing` until it is terminal, except while `parked` (no harness process).
A run over a limit waits in `queued` with `waiting_for = "concurrency"`.

## Agent variants

`[agents.<name>]` of the orchestrator or a project declares a pack agent under another name with settings of its own:
`base = "<pack>.<agent>"` and any keys of `agent.toml` but `prompt` (the role prompt, skills and directory are the
base agent's). A rig runs a variant in place of an agent the formulas name: `[rigs.<r>] agents = { <agent> =
"<variant>" }`. A run of the variant is of the variant's name: `max_runs` (its own, else the base agent's), the gate
`agent:<variant>`, the events and `explain` count it as an agent of its own. The formulas of the pack still name the
base agent; the other way to the same change, without a new name, is an edit (`[rigs.<r>.agents.<agent>]`).

A project's variant replaces an orchestrator's of the same name. Load errors: a `base` that resolves to no agent; a
`base` that names a variant (a variant is of a pack agent, one level); a rig that maps to a variant nobody declares; a
variant named like an agent of a pack of the project.

## Formula: `formulas/<file>.toml`

```toml
name = "implement"
description = "Implement the task of a bead"
worktree = true                     # optional (default false): each run works in a git worktree of its own
rounds = 3                          # optional (default 3): the most rounds a check may send the run through
internal = false                    # optional: true starts it only from a `formula` or `drain` step

[inputs]
task = { type = "task", required = true }   # task | string | integer | boolean

[[step]]
name = "implement"                  # optional (default: the position, "1", "2", ..)
kind = "ai"                     # ai: agent (whose prompt.template.md it gets); exec: command;
                                # drain, formula: formula; await: none
agent = "implementer"
timeout = "45m"                     # optional
model = "opus"                      # optional, ai steps only: layer 8, the launch settings `harness`,
                                    # `model`, `effort`
native.claude = { permission_mode = "dontAsk" }  # optional, ai steps only: layer 8, merged by key

[[step]]
name = "test"
kind = "exec"
command = ["cargo", "test"]
back_to = "implement"               # optional, ai, exec and await steps: a check; on fail, back to this earlier step
max_attempts = 2                    # optional, any step but await (default 1): the most times the step runs
step_check_script_path = "check-test.sh"  # optional, any step but await: passes when it exits 0; from this file's
                                          # directory

[[step]]
name = "children"
kind = "drain"
formula = "fix"                     # the formula of each bead's run
timeout = "24h"                     # optional (default 24h)
pool = 2                            # optional (default 1): the most beads in work at once
on_item_failure = "soft_stop"       # optional: continue (default) | soft_stop | hard_stop

[[step]]
name = "review"
kind = "formula"
formula = "review"                  # a formula of the project, run as a child run
inputs = { focus = "{{ steps.implement.summary }}" }  # optional: inputs of `formula`, given on top of inherited
timeout = "24h"                     # optional (default 24h)

[[step]]
name = "approve"
kind = "await"                      # an outcome from outside: a human, a script or a plugin
timeout = "168h"                    # optional (default 168h)
fail_on_timeout = true              # optional (default true): at the timeout the run fails; false: the step passes
back_to = "implement"               # optional: a check, as above
```

A formula has a name, a description, declared inputs and one or more steps, which a run goes through in order
(contracts 6.1): the next step starts when the one before ended, a failed step that is no check fails the run, and
the run's outcome is that of its last step. Step names are unique in the formula and contain no `.`. An `ai` step
is a turn of the agent in a harness session, and its outcome comes from the finisher (`aae finish`, contracts 6.3). An
`exec` step is a command with no model that the daemon runs in the run's workdir, in a process group of its own, under
the step timeout; exit 0 is `pass`, 75 (`EX_TEMPFAIL`) a failed attempt (not an outcome), any other `fail`, and a
bounded tail of its output is kept in the result (contracts 6.2). An `exec` step has `command = ["prog", "arg", ..]`
(non-empty) and `timeout` instead of `agent`. Nothing is substituted into the command; it gets the run in
its environment: `AAE_RUN`, `AAE_BEAD` (when the run has a bead), `AAE_SOCKET` (the API socket, so the command can
call `aae`) and `AAE_INPUT_<NAME>` per input (the name upper-cased, other characters than letters and digits as `_`; a
string as it is, any other value as JSON). Two inputs that give one variable (`note-text` and `note_text`) are a
config error.

A `drain` step gives every ready bead under the run's bead, at any depth, a run of `formula`, on each read of the
tracker, until none is left: the same start as `aae run start` with the bead as the formula's input of type `task`, no
rig named (the prefix of the bead's id gives it, the project root when it matches no rig, and its `agent:`/`harness:`
labels apply as to any run), the run's correlation, one level deeper, the run as `parent_run`. A bead goes out once
per entry of the step: a child that failed is not run again in it, and an attempt cut by a restart (`daemon_crash`)
starts no new entry; a round back to the step does. A bead whose start is refused (a `rig:` label, a `harness:` label
the config does not give) gets no run and stays open. The step has no process (contracts 6.2): it ends once nothing
was given, no child is alive and no later read of the tracker is queued, with `data = {closed, failed, refused}` — the
beads whose runs passed, those whose runs failed or were cancelled, and the refused ones with the reason. `pool`
(default 1) is the most beads of the step in work at once: a child holds its place from its creation, queued or
running, and the end of one gives out the next ready bead. `on_item_failure` says what a failed run of a bead does:
`continue` (default) — the other beads go on and the step passes; `soft_stop` — no new bead goes out, the started ones
end; `hard_stop` — the started ones are cancelled too (reason `drain_hard_stop`), on the read of the tracker that the
failed run's end queues. A stopped step ends `fail` once no child is alive, its `data` adds `stopped_on` (the failed
beads) and `not_started` (the ready beads under the root it did not give), and its summary is `stopped on <beads>: <n>
closed, <n> failed, <n> not started`. The stop follows from the children of the entry, so a restart keeps it: no new
bead goes out after one; a repeat of the step or a round back to it is a new entry and gives the beads again. A
cancelled child stops nothing. `formula` must take exactly one input of type `task` and need no other; a formula with
a `drain` step needs a required `task` input. An epic under the root is not ready while it has open beads; once they
are closed it is given out as any bead (with an id of no rig prefix: in the root).

A `formula` step runs `formula`, a formula of the run's project (a formula of another project is not in its reach,
whatever its name), as one child run: the same rig, the run's correlation, one level deeper, the run as
`parent_run`. The child's outcome (result, summary, data) is the step's. Its inputs are those of the parent it
declares (a `task` input too), and on top of them the step's `inputs`: a table as a trigger's, each string a template
over `inputs`, `steps`, `run`, `bead` and `project`. The child works on the bead its parent holds without owning it
(an explicit `task` naming that bead works the same way): it makes no claim and at its end neither closes nor releases
the bead, the parent does that at its own end; `bead` in the child's templates is the parent's snapshot of it, and a
child of that child may borrow it on. A child that cannot start (the start is refused, or `next_ready` finds no ready
bead) fails the parent at once with class `infrastructure`, with no count toward `max_attempts`; a parent that fails in
the step cancels its live child. The step has no process: it ends when the child does; a cancelled parent cancels the
child, and a restart finds the child it made and makes no second one. `max_attempts` and `step_check_script_path`
apply as to any step. A formula with `internal = true` runs only from such a step or a `drain` step: a start by
hand, from the API, MCP or a session is refused (`` `<name>` is internal: only a step of another formula runs
it ``), `GET /v1/formulas` marks each formula with `internal`, and
`aae formula list` hides the internal ones unless `--all`. Config errors: a step that neither gives nor inherits a
required input of its formula (``needs input `goal`, which the step neither gives nor inherits``), one that gives an
input the formula does not declare, a formula that runs itself through its `formula` steps, at any depth, and a
trigger on an internal formula.

An `await` step waits for an outcome given from outside, by a human, a script or a plugin, through
`POST /v1/runs/{id}/resolve` or `aae run resolve <run> <step> pass|fail` (contracts 6.3): `pass` or `fail`, a
summary and data, as the finisher's. Entering the step is the event `run.awaiting` with its deadline. The step has no
process and holds no concurrency slot; the wait is a state, not a timer of the daemon's: a restart leaves it as it
is, and its deadline is wall time. An outcome for another step, or for a run that does not wait on this one, is
refused and changes nothing. The outcome goes the way of any step's (the hooks on `run.finishing`): `pass` moves the
run on, `fail` with `back_to` is a failed check and `fail` without one fails the run (class `agent`, the summary as
the reason). At the `timeout` (default 168h) with no outcome the run fails with the reason `await_timeout` (class
`check`) and no return to `back_to`; with `fail_on_timeout = false` the step passes with the summary `timeout`. The
`timeout` and `fail_on_timeout` of a waiting step are those it entered with. An `await` step takes neither
`max_attempts` nor `step_check_script_path` (nor `agent`, `command`, `formula`): either is a load error.

A step with `back_to` is a check. When it fails the run does not fail: the steps after it that are checks too run
as well, and after the last of them every failed check of the row sends the run back together, to the earliest
step they name, which starts at once as the next round. An `await` step ends the row of checks before it: a run
reaches it only when they all passed, and failed ones send the run back first; the checks after an `await` step
belong to its row. Their remarks — per check its name, its summary and, for an
exec check, the output it kept — reach the steps of the new round as `{{ remarks }}` in a prompt template and
`AAE_REMARKS` in the environment of an exec step (empty and absent in the first round). An ai step of the new
round that already has a session goes on in it: its first attempt resumes the session and gets the
rendered prompt with the remarks, not the reconcile prompt of a resumed retry. A check that fails in the
last of the formula's `rounds` fails the run (class `agent`) with the reason `still failing after <n> rounds:
<remarks>`. `back_to` must name a step before the check; a `drain` step is no check.

Any step but `await` may have `max_attempts` (1 to 20, the ceiling of session attempts; default 1): the step runs at most that
many times, and one that does not pass runs again until then; past it the step fails. `step_check_script_path` names a script that says
whether the step passed, a path from the directory of the formula file (an absolute one stays; empty is none) that
must be an executable regular file at load. Its exit code decides: 0 passed, 75 an incurable failure that fails the
step with no more runs, any other code did not pass. Unlike the exit code 75 of an `exec` step's command, a failed
attempt, 75 of the check script is final: it fails the run at once, and a check step with `back_to` sends nothing
back for it, since an incurable failure is no work for an earlier step. A script that outlives the step timeout, dies
by a signal or whose answer is lost (the deadline of `finishing`) did not pass; one that cannot start fails the run
as `infrastructure`. The script runs after the step has run on its own and its hooks accepted
`pass`; a step that failed on its own (finisher `fail`, a non-zero exit of the command) is a run that did not pass,
and the script is not started. It gets the environment of an `exec` step plus `AAE_STEP_<NAME>_RESULT`, `_SUMMARY`
and `_DATA` of its own step, in the run's workdir under the step timeout. A repeat of an `ai` step goes on in its
session and gets the script's output as a remark: through `{{ remarks }}`, or appended to the prompt when the
template has none. A repeat of a `drain` step is a new entry. The count is within a round: a round back to the step
gives it the full `max_attempts` again. A repeat spends neither the session attempt ceiling (invariant 16,
[invariants.md](invariants.md#the-invariants)) nor the rounds, and a failure of the infrastructure does not spend
`max_attempts`.

A step sees the outcomes of the steps before it, by name (the last attempt of each): a prompt template as
`{{ steps.<name>.result }}`, `.summary` and `.data.<key>` (contracts 2.3; `data` of an exec step is `{exit_code,
stdout, stderr}`), an exec step as `AAE_STEP_<NAME>_RESULT` (`pass` or `fail`), `AAE_STEP_<NAME>_SUMMARY` and
`AAE_STEP_<NAME>_DATA` (JSON, when there is data), `<NAME>` formed as for inputs; a step name whose variables collide
with another step's or an input's is a config error. Each ai step has a session of its own; a run that continues a
session (`continue_from`) continues it in its first step only, so a formula whose first step is not `ai` cannot be
continued.

A run start checks the inputs: no undeclared one, every `required` one present (`null` is no value), an `integer` input
a JSON integer or its text, a `boolean` one a JSON boolean or `true`/`false` as text, a `string` one a string (the CLI
sends `--input` values as text). A `task` input (there is no input type `bead`: nobody files a bead for the user beforehand) is a string or
`{ next_ready = true, epic = <id> }`. A string that is the id of a bead has the run take that bead up; any other text
(not blank) has the tracker create the run's root bead as the run prepares, titled with the first line of the text and
described by all of it; either way the input then holds the bead id ([tracker-br.md](tracker-br.md#beads-and-runs)). A bead named
by id that belongs to another rig, by the prefix of its id, is refused at the start if the core has seen it, else at
its claim (the run fails with `bead_of_another_rig` and the claim is released). `worktree = true` gives each run of the formula, of either step kind,
a git worktree that aae makes and removes (the bundled `worktree`, see below). aae does not serialize runs by
directory: without a worktree, runs of one rig share its directory, and keeping them apart is the user's choice
(`worktree`, `pool`, `max_runs`).

A prompt template is checked at load against each formula whose steps name its agent (contracts 2.3):
`inputs.<name>` must be a declared input; `bead.<field>` needs a `task` input and a field of `Bead` (contracts 5.2); `run.<field>` a field of `RunView` (1.3); `project.name`,
`project.path` (the directory of `aae.toml`); `rig.name`, `rig.path` and `rig.<var>` (a variable of the run's rig,
checked when the run starts); `agent.<name>` (a file of the agent, see "Agent").
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
  of the daemon neither loses nor doubles a firing ([contracts 3.5](contracts.md#35-outbox-lifetime)).
- Overlap: while a run started by the same trigger with the same inputs is alive, a new firing is skipped with
  `trigger.skipped{overlap}`.
- A run started by a trigger inherits the correlation of its event. A tracker event carries the correlation of the
  run whose attempt made the write, known from the br actor `aae:<run>[:<attempt>]`; a write by a human starts a
  new chain ([contracts 5](contracts.md#5-tracker-interface)).
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

[[hook]]                            # orchestrator hook (contracts 3)
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

The plugins bundled with aae run in process as hooks of the contract of contracts 3, after every
declared plugin, so a declared plugin decides first. `enabled` (boolean) turns one on or off; every other key is a
parameter, checked when the daemon starts. A table with parameters and no `enabled` turns the plugin on. A
project's `aae.toml` may have `[builtin.<name>]` tables too: for the runs of that project such a table replaces the
orchestrator's table of that plugin whole, since the orchestrator side of a plugin is turned on for the orchestrator
or for a project.

- `default-policy` (on unless `enabled = false`) decides on `run.attempt_failed` and `run.turn_ended_without_outcome` as
  in contracts 10. Parameters, all integers: `quota_margin_s` (60), `quota_wait_s` (1800; the wait of `quota_exhausted`
  when neither `retry_at` nor the account's `resets_at` is known; an account is used up when `rate_limit.utilization >=
  1.0` and `resets_at` is in the future), `model_pause_s` (600), `backoff_base_s` (10), `backoff_cap_s` (300),
  `backoff_park_from` (3), `auth_backoff_base_s` (30), `auth_backoff_cap_s` (3600), `rapid_s` (30), `crash_loop` (5),
  `crash_pause_s` (300), `resume_failures` (3), `unknown_after_s` (30), `unknown_retries` (3). The waits and caps that a
  zero would turn into a hot loop (`quota_wait_s`, `model_pause_s`, both bases and both caps, `crash_pause_s`) and
  `crash_loop` must be at least 1; `quota_margin_s`, `rapid_s` and `unknown_after_s` must not be negative.
- `worktree` has no table: it is on for the runs of the formulas with `worktree = true` (a formula setting; there is no central
  list and no agent-level switch), and a `[builtin.worktree]` table is a config error saying so. Each such run gets a git worktree of its
  rig on the branch `aae/<run>`, in `<git common dir>/aae/worktrees/<run>`, as its workdir. When the run ends the
  worktree is removed unless it has uncommitted changes or a HEAD no ref reaches (then `plugin.hook_failed` says why);
  the branch is deleted only when it is merged. A run with `continue_from` starts in the workdir of the run it continues
  (a session resumes where it ran); when that is a worktree of this plugin, it is brought back on its branch if it was
  removed, whatever the run's formula says. A reload that leaves a project no formula with `worktree = true` turns the
  plugin off there: the worktrees of its runs still going are left in place, as are their branches. Runs of one
  repository that start or end at once make and remove their worktrees one at a time, under an OS lock on the git
  common dir: two `git worktree add` at once can race on the repository config. Its hook has the 30 s timeout of a
  hook, the wait for that lock included; a `git` still running at that timeout is killed with its process group and
  the hook fails.

### Where the bundled plugins end and the core begins

The mechanisms live in the core: resuming an attempt, a new session that keeps ownership, issue gates, the counters
the policy reads, the ceiling of 20 failed attempts per run (waits on an account gate not counted), the watchdog and
every deadline. The bundled plugins only choose a reaction and its thresholds; they never change a harness. With
`default-policy` off, the "no plugin decision" column of [contracts 1.3](contracts.md#13-payloads) applies: a failed
attempt ends the run `failed` with class `infrastructure`. The bundled plugins are compiled into the same binary, need
no external runtime, and are counted on their own budget line (`plugins` in `scripts/budget.sh`, 1500 lines) inside the
release total.

An `auth_required` or `billing` failure may be a passing error of a proxy or WAF (a 403 that goes away by itself)
rather than a real logout, so every such failure parks the account gate for a wait that grows with each failure in a row up to an hour, and the next
attempt is the probe. Nothing goes to an operator and there is no alerting: the journal and the logs tell it. The core
does not count these attempts against its ceiling, so a long outage never ends in `failed{attempts_ceiling}`. The raw
error stays in `failure.raw`. Not done: telling a real logout from a WAF page by the raw error; a real logout is probed
once an hour until someone logs in.

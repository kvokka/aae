# Frontends

The daemon has three frontends over one typed interface of the core: the HTTP API, the CLI and a small MCP server.
None of them holds state or domain logic of its own: each translates a request into a call of the core and its
answer back; the core's state and journal are the truth (docs/design/event-model.md).

## HTTP API

Code: `crates/aae-front/src/api/`. Reference: `docs/api/openapi.json`.

- **Transport.** A unix socket by default (`AAE_SOCKET` names it for clients). TCP only when the orchestrator config
  sets `api_tcp`; the address must be loopback, and every request on it needs `Authorization: Bearer <token>`. The
  token is new at every daemon start and is written to `api.token` in the state directory, readable by the owner
  only. `api_tcp` cannot change on a reload.
- **Format.** JSON in and out; errors are JSON with a code (`bad_request`, `not_found`, `rejected`, ...). A malformed
  gate key is `bad_request` (400).
- **Idempotency.** Every mutating operation takes an `Idempotency-Key` header; a repeated key returns the first
  answer. The key repeated with another method, path or body is refused with 422 `idempotency_key_reused` and
  changes nothing. Keys are kept a day. `POST /v1/config/check` changes nothing and takes none.
- **OpenAPI.** Generated from the code (`utoipa`), with the reply schemas and the error statuses of every
  operation (every error body is an `ErrorReply`); `crates/aae-front/tests/openapi.rs` fails when the committed
  document differs or an operation leaves out an error its kind can return. Regenerate with
  `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`.
- **Events** (`GET /v1/events`): SSE from a `seq` (or `Last-Event-ID`). A `seq` the journal rotated away is a 410
  `gap` reply (with `oldest`) before the stream starts, and a `gap` event when it is rotated away under a slow
  stream (a failed read of the journal ends the stream with a `journal` event); the client then re-reads the state
  over the API.
- **Live view of a run** (`GET /v1/runs/{id}/live`): the run's events from a `seq`, interleaved with the raw messages
  of its current session as `harness` events, relayed from the adapter's tap and never stored by the core. A reader
  that falls behind the tap's buffer (256 messages) gets `lagged` with `{"missed": n}` and goes on. The stream ends
  once it has read past the run's terminal event, and when the client leaves. A later attempt's session is picked up
  as it starts.

The surface: projects with their root and rigs (`GET /v1/projects`: `name`, `dir` — the directory of `aae.toml`,
`root` and `rigs`, each `{name, path, vars}`; the root is not among the rigs); agents (`GET /v1/agents` gives each
agent's harness in the project outside any rig and formula, `null` when its layers do not set one), formulas, packs,
plugins (list), config check, explain and reload; runs
(start, list, state, live view, cancel, answer a request, finish); triggers (list, enable, disable, fire by hand);
instances and services (list, stop an idle one); issue gates (list, close, open — orchestrator, project, agent, bead,
account); events; daemon status, drain and planned restart.

## CLI

Code: `crates/aae-front/src/cli/`. The CLI is a thin client of the HTTP API in the same binary.

- One command per API operation; a test checks the command table against the OpenAPI document.
- `--json` on every command gives the API's JSON; `--key` on every mutating command sets the idempotency key.
- An error exits 1 with `aae: <message>` on stderr; under `--json`, stderr is `{"error": {"code", "message", ...}}`:
  the API's error object, or for a local error the code `no_daemon`, `usage`, `config_error`, `not_found` or
  `internal`. Arguments that do not parse exit 2 with the usage text.
- `aae run start <project> [<rig>] <formula>` (options after the positionals) prints the run id; without the rig
  the run takes the rig of the id prefix of the bead its task names, else works in the project root. When a task
  input `{"next_ready": true}` finds nothing ready it says so on
  stderr and exits 3. Inside an agent session (`AAE_RUN` and `AAE_ATTEMPT_TOKEN` set, as for `aae finish`) it sends
  them as `parent` of `POST /v1/runs`: the run is a child of the session's run, with its correlation and one level
  deeper, at most 10 ([contracts](../design/contracts.md#61-states)); a token that is not that of the parent's live
  current attempt is `rejected` ("stale attempt"). From a shell of its own the run starts a new correlation at depth 0.
  `aae run list --parent <run>` lists a run's children; cancelling a run cancels the live runs its sessions
  started (reason `parent_cancelled`), while its pass or failure leaves them alone.
- `GET /v1/formulas` gives every formula with `internal` (bool): an internal formula only starts as a step of
  another formula, not directly. `aae formula list` leaves the internal ones out; `--all` shows them too.
- `aae run watch <run>` prints the live view (a line per event, `-  harness  <type>` per session message) and exits
  0 once the stream ends with the run; `aae run wait` gives the outcome.
- `aae run wait <run>... [--timeout <dur>]` waits on the event stream (not by polling) until every named run is
  terminal, then prints for each `id`, `state`, `class`, `reason` and the finisher's `summary` and `data` (with
  `--json` an array of these objects). A run already terminal is answered at once. Exit codes: 0 when all passed,
  1 when one did not (or a run is unknown), 124 when the timeout ran out first; then the current state of each run is
  printed. A daemon restart in between is reconnected to with backoff and the states are read again.
- An agent that waits for its children holds its turn open on one long tool call, and two clocks watch it. The
  watchdog's silence budget while a tool runs is `[watchdog] tool_silence` (default 10 min); each
  `Activity{tool_running}` from the adapter renews it. Claude sends a `tool_progress` heartbeat every 30 s, so a
  long `aae run wait` in Bash never trips it, but claude moves a foreground Bash call to the background after 120 s
  (`BASH_DEFAULT_TIMEOUT_MS`). Codex has no heartbeat: only the start of a command and its output renew the budget,
  and `aae run wait` prints nothing until it ends, so a wait longer than `tool_silence` fails the attempt as
  `stall`. Either way, wait in a loop of `aae run wait <runs> --timeout 100s` (exit 124: wait again), or raise
  `tool_silence` above the longest wait. The whole wait still counts against the step's `timeout` (default 45 min).
- `aae finish` reads the attempt token from the session environment, retries for 60 s under one idempotency key and
  exits 75 when the daemon stays unreachable (an attempt failure, not a failed run).
- Without a daemon only `aae daemon`, `aae doctor`, `aae cache clear`, `aae config check|explain` and `aae --version`
  work; `aae cache clear` removes the packs and plugins downloaded from git, and the next load of the config downloads
  them again;
  any other command says that no daemon listens on the socket and how to start one. `aae config check` and
  `aae config explain` load the config file themselves as the daemon would (`--config <file>`, default
  `$XDG_CONFIG_HOME/aae/config.toml`) and print what the API would answer for it; `explain --formula` without `--rig` is of a run in the project root.
- `aae doctor` checks the host for a daemon that may not run yet (harness and br binaries, logins, private
  directories, the state file), from the shell that will start it. `GET /v1/doctor` makes the same checks from the
  running daemon's process, with its environment and config file; a failing check is an `error` level in the reply
  (200), as in `aae doctor --json`. The daemon's own state is in `GET /v1/daemon` (journal, outbox, runs in a turn).
- A refused `aae config reload` prints the errors of the file, one per line.

## MCP

Code: `crates/aae-front/src/api/mcp.rs`; details in `docs/api/mcp.md`. Five tools only (`list_formulas`,
`start_run`, `run_state`, `list_runs`, `cancel_run`), over the same handlers as the API, served on `/mcp` of the
API listeners in both protocol epochs. No agent gets it by default: tool descriptions cost tokens in every session,
so an agent names it in its own config (`[mcp.aae] url = "${AAE_API}/mcp"`, which needs `api_tcp`).

## Humans and runs

- A human sees runs, follows a run's live view and answers its permission requests and questions through these
  frontends (`aae run watch`, `aae run answer` or the API).
- The state of a run shows each attempt's `session`: the harness's own session reference (claude
  `{session_id, config_dir, workdir}`, codex `{thread_id, codex_home}`). After the run a human may open that conversation
  with the harness's own tools; aae takes no part in it.
- Joining a live session and handing control over to a human are not in the first release: it has no operation, no
  state and no stub for them, and the driver interface has none either
  ([contracts](../design/contracts.md#41-shape)).

## Verified by

- Integration tests on the real daemon: `crates/aae/tests/api.rs`, `cli.rs`, `mcp.rs`; the live view and
  `run watch` on the real claude (`crates/aae/tests/claude.rs`); the `lagged` path on a real broadcast channel
  (`api/live.rs`).
- The tests of acceptance criterion 17 (a CLI command per operation, OpenAPI equal to the code, five MCP tools) run in
  `mise run ci`. The live scenario `c17_mcp.rs` has a real haiku session call `list_formulas` and report a formula only the tool named.

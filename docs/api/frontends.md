# Frontends

The daemon has three frontends over one typed interface of the core: the HTTP API, the CLI and a small MCP server.
None of them holds state or domain logic of its own: each translates a request into a call of the core and its
answer back; the core's state and journal are the truth (docs/design/event-model.md).

## HTTP API

Code: `crates/loop-manager-front/src/api/`. Reference: `docs/api/openapi.json`.

- **Transport.** A unix socket by default. `LOOP_MANAGER_SOCKET` names it for clients and for `loop-manager daemon`
  alike: a daemon started from inside a loop-manager session, where the variable is set, needs its own `--socket`, or
  it refuses to start (another daemon is listening there). TCP only when the orchestrator
  config sets `api_tcp`; the address must be loopback, and every request on it needs `Authorization: Bearer <token>`.
  The token is new at every daemon start and is written to `api.token` in the state directory, readable by the owner
  only. `api_tcp` cannot change on a reload.
- **Format.** JSON in and out; errors are JSON with a code (`bad_request`, `not_found`, `rejected`, ...). A request
  that names a run, instance, trigger, project, rig or formula there is none of is 404 `not_found` on any method (the finisher
  answers `invalid` for an unknown run); 409 `rejected` is a thing that is there refusing the request as it stands. A
  malformed gate key is `bad_request` (400).
- **Idempotency.** Every mutating operation takes an `Idempotency-Key` header; a repeated key returns the first
  answer. The key repeated with another method, path or body is refused with 422 `idempotency_key_reused` and
  changes nothing. Keys are kept a day. `POST /v1/config/check` changes nothing and takes none.
- **OpenAPI.** Generated from the code (`utoipa`), with the reply schemas and the error statuses of every
  operation (every error body is an `ErrorReply`); `crates/loop-manager-front/tests/openapi.rs` fails when the committed
  document differs or an operation leaves out an error its kind can return. Regenerate with
  `LOOP_MANAGER_UPDATE_OPENAPI=1 cargo nextest run -p loop-manager-front openapi`.
- **Events** (`GET /v1/events`): SSE from a `seq` (or `Last-Event-ID`). A `seq` the journal rotated away is a 410
  `gap` reply (with `oldest`) before the stream starts, and a `gap` event when it is rotated away under a slow
  stream (a failed read of the journal ends the stream with a `journal` event); the client then re-reads the state
  over the API.
- **Live view of a run** (`GET /v1/runs/{id}/live`): the run's events from a `seq`, interleaved with the raw messages
  of its current session as `harness` events, relayed from the adapter's tap and never stored by the core. A reader
  that falls behind the tap's buffer (256 messages) gets `lagged` with `{"missed": n}` and goes on. The stream ends
  once it has read past the run's terminal event, and when the client leaves. A later attempt's session is picked up
  as it starts.

The surface: projects with their root and rigs (`GET /v1/projects`: `name`, `dir` — the directory of
`loop-manager.toml`, `root` and `rigs`, each `{name, path, vars}`; the root is not among the rigs); agents
(`GET /v1/agents` gives each agent's harness in the project outside any rig and formula, `null` when its layers do not
set one, and the call graph there: `subagents`, the agents and variants its session may call, and `called_by`, the
agents that may call it, the agent itself left out of both; `loop-manager agent list` includes both arrays),
formulas, packs, addons (list), config check, explain and reload; runs
(start, list, state, live view, cancel, finish, resolve); triggers (list, enable, disable, fire by hand);
instances and services (list, stop an idle one); issue gates (list, close, open — orchestrator, project, agent, bead,
account); events; daemon status, drain and planned restart.

## CLI

Code: `crates/loop-manager-front/src/cli/`. The CLI is a thin client of the HTTP API in the same binary.

- One command per API operation; a test checks the command table against the OpenAPI document.
- Structured replies use TOON by default, including when stdout is piped. The CLI encodes them in-process with the
  same encoder as MCP tools. Pass the global `--json` flag before or after the subcommand to request JSON, for
  example `loop-manager run list --json | jq '.[].id'`.
- API and local client errors print an `error` object on stderr in the selected format. It contains
  `code`, `message` and any details from the daemon. The usual exit code is 1. Arguments that do not parse exit 2
  with the usage text.
- `--key` on a mutating command sets the idempotency key.
- `loop-manager run start <project> [<rig>] <formula>` (options after the positionals) prints the bare run id.
  With `--json`, read the id from the reply's `run` field. Without the rig the run takes the rig of the id prefix of
  the bead its task names, else works in the project root. When a task input `{"next_ready": true}` finds nothing
  ready it exits 3, with a message on stderr by default or `"nothing_ready": true` in the JSON reply. Inside an agent
  session (`LOOP_MANAGER_RUN` and `LOOP_MANAGER_ATTEMPT_TOKEN` set, as for `loop-manager finish`) it sends them as
  `parent` of `POST /v1/runs`: the run is a child of the session's run, with its
  correlation and one level deeper, at most 10 ([contracts](../design/contracts.md#61-states)); a token that is not that
  of the parent's live current attempt is `rejected` ("stale attempt"). From a shell of its own the run starts a new
  correlation at depth 0. `loop-manager run list --parent <run>` lists a run's children; cancelling a run cancels the
  live runs its sessions started (reason `parent_cancelled`), while its pass or failure leaves them alone.
- `GET /v1/formulas` gives every formula with `internal` (bool): an internal formula only starts as a step of
  another formula, not directly. `loop-manager formula list` leaves the internal ones out; `--all` shows them too.
- `loop-manager events` and `loop-manager run watch <run>` print each event as it arrives, with a blank line between
  TOON documents. Journal events include their `seq`, `type` and `data`. Events outside the journal (such as
  `harness` and `lagged`) have `event` and `data` fields. With `--json`, each line contains the original SSE data as
  a JSON value. A watch exits 0 when the run's stream ends. Use `loop-manager run wait` for the outcome.
- `loop-manager run wait <run>... [--timeout <dur>]` waits on the event stream (not by polling) until every named run is
  terminal, then prints for each `id`, `state`, `class`, `reason` and the finisher's `summary` and `data` (with
  `--json` an array of these objects). A run already terminal is answered at once. Exit codes: 0 when all passed,
  1 when one did not (or a run is unknown), 124 when the timeout ran out first; then the current state of each run is
  printed. A daemon restart in between is reconnected to with backoff and the states are read again.
- An agent that waits for its children holds its turn open on one long tool call, and two clocks watch it. The
  watchdog's silence budget while a tool runs is `[watchdog] tool_silence` (default 40 min); each
  `Activity{tool_running}` from the adapter renews it. Claude sends a `tool_progress` heartbeat every 30 s, so a
  long `loop-manager run wait` in Bash never trips it, but claude moves a foreground Bash call to the background after
  120 s (`BASH_DEFAULT_TIMEOUT_MS`). Codex has no heartbeat: only the start of a command and its output renew the
  budget, and `loop-manager run wait` prints nothing until it ends, so a wait longer than `tool_silence` fails the
  attempt as `stall`. Either way, wait in a loop of `loop-manager run wait <runs> --timeout 100s` (exit 124: wait
  again), or raise `tool_silence` above the longest wait. The whole wait still counts against the step's `timeout`
  (default 4 h for an `ai` step).
- `loop-manager run resolve <run> <step> pass|fail [--summary <text>] [--data <json>|@<file>]`
  (`POST /v1/runs/{id}/resolve`) gives the outcome of the `await` step a run waits on
  ([formula](../design/config.md#formula-formulasfiletoml)); a run that does not wait on that step refuses it with
  `rejected` and nothing changes. `loop-manager run list --awaiting` (`GET /v1/runs?awaiting=true`) lists the runs
  waiting on an `await` step. The `step` field names the current step of each run, and the state of a run
  gives the outcomes of the steps it went through as `steps`. A bridge that may send an outcome twice sets `--key`:
  without a key, a late repeat could resolve the step again after a round brought the run back to it.
- `loop-manager finish` reads the attempt token from the session environment and prints the acknowledgement
  (for example `accepted`), or the object with the `answer` field under `--json`. It retries for 60 s under one
  idempotency key and exits 75 when the daemon stays unreachable.
- `loop-manager format` reads one JSON value from stdin and writes TOON to stdout. For example,
  `loop-manager format < report.json` formats a report assembled by a script. Invalid input exits 1.
- Without a daemon only `loop-manager format`, `loop-manager daemon`, `loop-manager cache clear`, `loop-manager --version`, and
  `loop-manager doctor` and `loop-manager config check|explain` with `--config <file>` work; `loop-manager cache clear`
  removes the packs and addons downloaded from git, and the next load of the config downloads them again; any other
  command says that no daemon listens on the socket and how to start one, and for `doctor` and `config check|explain`
  that `--config <file>` checks a file without one.
- `loop-manager config check`, `loop-manager config explain` and `loop-manager doctor` ask the daemon
  (`POST /v1/config/check`, `GET /v1/config/explain`, `GET /v1/doctor`), so they answer for the config, environment and
  `PATH` the daemon runs with, whatever shell they are typed in. With `--config <file>` they load that file themselves
  as the daemon would and make the checks from this shell, with no daemon: how a file is checked before a daemon first
  runs on it. Both ways print the same reply the same way and exit alike: non-zero on a config error or an `error` level
  check. A config check writes `ok` and `errors` to stdout. An explain prints the configuration report as text, or
  its JSON envelope with `--json`. `explain --formula` without `--rig` is of a run in the project root.
  The daemon's own state is in `GET /v1/daemon` (journal, outbox, runs in a turn).
- A config reload that fails validation includes the file's errors in `error.errors` on stderr.

## MCP

Code: `crates/loop-manager-front/src/api/mcp.rs`; details in `docs/api/mcp.md`. Five tools only (`list_formulas`,
`start_run`, `run_state`, `list_runs`, `cancel_run`), over the same handlers as the API, their replies as TOON text,
served on `/mcp` of the API listeners in both protocol epochs. No agent gets it by default: tool descriptions cost
tokens in every session, so an agent names it in its own config
(`[mcp.loop-manager] url = "${LOOP_MANAGER_API}/mcp"`, which needs `api_tcp`).

## Humans and runs

- A human sees runs and follows a run's live view through these frontends (`loop-manager run watch` or the API), and
  gives the outcome of an `await` step (`loop-manager run resolve`), by hand or through a bridge from another system
  (demo 3's `demos/3-review-gate/bridge.sh` from GitHub, with an idempotency key per round); the skill `awaiting` of the
  loop-manager plugin for Claude Code and Codex (`plugins/loop-manager/`) shows a user's agent the runs that wait and passes on the
  user's outcome or cancel. A human answers no permission request: the harness decides them from the config
  ([config](../design/config.md#permissions)).
- The state of a run shows each attempt's `session`: the harness's own session reference (claude
  `{session_id, config_dir, workdir}`, codex `{thread_id, codex_home}`). After the run a human may open that conversation
  with the harness's own tools; loop-manager takes no part in it.
- A run that failed for good is recovered by hand with these commands: continue its session, start a new one on its
  bead with or without a note, or abandon the bead ([recovering a failed run](../guide/recovery.md)).
- Joining a live session and handing control over to a human are not in the first release: it has no operation, no
  state and no stub for them, and the driver interface has none either
  ([contracts](../design/contracts.md#41-shape)).

## Verified by

- Integration tests on the real daemon: `crates/loop-manager/tests/e2e/api.rs`, `cli.rs`, `mcp.rs`; the live view and
  `run watch` on the real claude (`crates/loop-manager/tests/e2e/claude.rs`); the `lagged` path on a real broadcast
  channel (`api/live.rs`).
- The tests of acceptance criterion 17 (a CLI command per operation, OpenAPI equal to the code, five MCP tools) run in
  `mise run ci`. The live scenario `c17_mcp.rs` has a real haiku session call `list_formulas` and report a formula only the tool named.

## Expanded run card in chat

Install `run-card@loop-manager` from the [operator plugin marketplace](../../plugins/README.md).
In an MCP Apps host, ask to show the expanded card for a run ID. Its formula stays in place while the
current step, round, step attempt and wait reason change. A dotted arrow marks a possible return;
the highlighted arrows mark the checks in the run's `back_from`. Click the heading to collapse the card.

The host starts the plugin over stdio. It reads the run and the ordered steps and `back_to` edges of the
currently configured formula. If the formula was removed, it shows the current step alone. The card
follows the journal cursor over the daemon's Unix socket, refreshes its snapshot after a gap, reconnects
on failure and stops following at a terminal state. Live refresh uses the host's MCP Apps bridge.
Clients with text output get a status line and can follow the run with `loop-manager run watch <run>`.

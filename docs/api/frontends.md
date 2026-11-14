# Frontends

The daemon has three frontends over one typed interface of the core: the HTTP API, the CLI and a small MCP server.
None of them holds state of its own; the core's state and journal are the truth (docs/design/event-model.md).

## HTTP API

Code: `crates/aae-front/src/api/`. Reference: `docs/api/openapi.json`.

- **Transport.** A unix socket by default (`AAE_SOCKET` names it for clients). TCP only when the orchestrator config
  sets `api_tcp`; the address must be loopback, and every request on it needs `Authorization: Bearer <token>`. The
  token is new at every daemon start and is written to `api.token` in the state directory, readable by the owner
  only. `api_tcp` cannot change on a reload.
- **Format.** JSON in and out; errors are JSON with a code (`bad_request`, `not_found`, `rejected`, ...). A malformed gate
  key is `bad_request` (400).
- **Idempotency.** Every mutating operation takes an `Idempotency-Key` header; a repeated key returns the first
  answer. The key repeated with another method, path or body is refused with 422 `idempotency_key_reused` and
  changes nothing. Keys are kept a day. `POST /v1/config/check` changes nothing and takes none.
- **OpenAPI.** Generated from the code (`utoipa`), with the reply schemas and the error statuses of every
  operation (every error body is an `ErrorReply`); `crates/aae-front/tests/openapi.rs` fails when the committed
  document differs or an operation leaves out an error its kind can return. Regenerate with
  `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`.
- **Events** (`GET /v1/events`): SSE from a `seq` (or `Last-Event-ID`). A `seq` the journal rotated away is an
  explicit `gap` event before the stream starts; the client then re-reads the state over the API.
- **Live view of a run** (`GET /v1/runs/{id}/live`): the run's events from a `seq`, interleaved with the raw messages
  of its current session as `harness` events, relayed from the adapter's tap and never stored by the core. A reader
  that falls behind the tap's buffer (256 messages) gets `lagged` with `{"missed": n}` and goes on. The stream ends
  once it has read past the run's terminal event, and when the client leaves. A later attempt's session is picked up
  as it starts.

The surface: projects and rigs; agents, formulas, packs, plugins (list), config check, explain and reload; runs
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
- `aae run start` prints the run id; when a bead input finds nothing ready it says so on stderr and exits 3.
- `aae run watch <run>` prints the live view (a line per event, `-  harness  <type>` per session message) and exits
  0 once the stream ends with the run; `aae run show` gives the outcome.
- `aae finish` reads the attempt token from the session environment, retries for 60 s under one idempotency key and
  exits 75 when the daemon stays unreachable (an attempt failure, not a failed run).
- Without a daemon only `aae daemon`, `aae doctor`, `aae fetch`, `aae config check|explain` and `aae --version` work;
  any other command says that no daemon listens on the socket and how to start one. `aae config check` and
  `aae config explain` load the config file themselves as the daemon would (`--config <file>`, default
  `$XDG_CONFIG_HOME/aae/config.toml`) and print what the API would answer for it.
- A refused `aae config reload` prints the errors of the file, one per line.

## MCP

Code: `crates/aae-front/src/api/mcp.rs`; details in `docs/api/mcp.md`. Five tools only (`list_formulas`,
`start_run`, `run_state`, `list_runs`, `cancel_run`), over the same handlers as the API, served on `/mcp` of the
API listeners in both protocol epochs. No agent gets it by default: tool descriptions cost tokens in every session,
so an agent names it in its own config (`[mcp.aae] url = "${AAE_API}/mcp"`, which needs `api_tcp`).

## Verified by

- Integration tests on the real daemon: `crates/aae/tests/api.rs`, `cli.rs`, `mcp.rs`; the live view and
  `run watch` on the real claude (`crates/aae/tests/claude.rs`); the `lagged` path on a real broadcast channel
  (`api/live.rs`).
- Criterion 17 (a CLI command per operation, OpenAPI equal to the code, five MCP tools) runs in CI. The request
  scenario `c17_mcp.rs` has a real haiku session call `list_formulas` and report a formula only the tool named.

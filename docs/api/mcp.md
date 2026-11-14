# MCP frontend

The daemon serves a small MCP server on its API socket at `/mcp` (spec §12). It is connected to no agent by
default: tool descriptions cost tokens in every session, so an agent gets it only by naming it in its own config.

A harness reaches it over TCP: set `api_tcp = "127.0.0.1:<port>"` in the orchestrator config and give the agent
`[mcp.aae] url = "${AAE_API}/mcp"`; the session gets the URL and the bearer token (docs/design/config.md). Every
request on the TCP listener needs `Authorization: Bearer <token>`; the daemon writes a new token at every start to
`api.token` in its state directory, readable by the owner only. The unix socket needs no token.

## Tools

| Tool | Arguments | Same as |
|---|---|---|
| `list_formulas` | `project`? | the formulas of the config, named as `start_run` takes them, with their inputs |
| `start_run` | `project`, `rig`, `formula`, `inputs`?, `continue_from`?, `idempotency_key`? | `POST /v1/runs` |
| `run_state` | `id` | `GET /v1/runs/{id}` |
| `list_runs` | `project`?, `state`?, `limit`? | `GET /v1/runs` |
| `cancel_run` | `id`, `reason`?, `idempotency_key`? | `POST /v1/runs/{id}/cancel` |

A tool result carries the API's JSON reply as text. An API error (`bad_request`, `not_found`, `rejected`, ...) is a
result with `isError: true` and the text `<code>: <message>`; an unknown tool is the JSON-RPC error `-32602`. `idempotency_key` works as the API's `Idempotency-Key`: the same key
and arguments return the first result, other arguments are `idempotency_key_reused`.

## Protocol epochs

Both epochs are served (spec §11.4), chosen per request:

- `MCP-Protocol-Version: 2026-07-28` — stateless: `server/discover`, `tools/list`, `tools/call`; results carry
  `resultType: "complete"`.
- Otherwise the `initialize` epoch: `initialize` answers the requested version if it is one of 2025-11-25,
  2025-06-18, 2025-03-26 (else 2025-11-25), `notifications/initialized` gets 202. No `Mcp-Session-Id` is issued,
  so a daemon restart never makes a client's session unknown. `server/discover` here is `-32601`, the answer on
  which claude falls back to `initialize`.

`GET /mcp` is 405: the server opens no stream of its own. One JSON-RPC message per request (no batches).

# MCP frontend

The daemon serves a small MCP server on its API socket at `/mcp`. It is connected to no agent by default: tool
descriptions cost tokens in every session, so an agent gets it only by naming it in its own config. For the same
reason the set stays at the five tools below: an agent that needs more, a planner for one, uses the CLI.

A harness reaches it over TCP: set `api_tcp = "127.0.0.1:<port>"` in the orchestrator config and give the agent
`[mcp.loop-manager] url = "${LOOP_MANAGER_API}/mcp"`; the session gets the URL and the bearer token
(docs/design/config.md). Every request on the TCP listener needs `Authorization: Bearer <token>`; the daemon writes a
new token at every start to `api.token` in its state directory, readable by the owner only. The unix socket needs no
token.

## Tools

| Tool | Arguments | Same as |
|---|---|---|
| `list_formulas` | `project`? | the formulas of the config a user can start, named as `start_run` takes them, with their inputs; internal ones are left out |
| `start_run` | `project`, `rig`? (when absent: the prefix of the bead's id, else the project root), `formula`, `inputs`?, `continue_from`?, `idempotency_key`? | `POST /v1/runs` |
| `run_state` | `id` | `GET /v1/runs/{id}` |
| `list_runs` | `project`?, `state`?, `limit`? | `GET /v1/runs` |
| `cancel_run` | `id`, `reason`?, `idempotency_key`? | `POST /v1/runs/{id}/cancel` |

An MCP session carries no attempt of an agent session, so `start_run` starts a run at depth 0 with a new
correlation; an agent starts a child of its own run with `loop-manager run start` from its session's shell
([frontends](frontends.md#cli)).

A tool result carries the API's reply as text, in TOON rather than JSON: it goes into the agent's context, where
TOON costs fewer tokens ([contracts](../design/contracts.md), the TOON forms loop-manager writes). The result has no
`structuredContent`. `start_run` answers, for one:

```text
run: run_1
```

and `run_state` a run view, its scalars as fields, its attempts as a table:

```text
agent: null
attempt: 1
attempts[1]{ended_at,failure_kind,generation,harness,input_tokens,kind,n,output_tokens,session,started_at,step}:
  null,null,1,exec,0,new_session,1,0,null,"2026-11-19T04:52:09.427Z","1"
...
state: running
```

An API error (`bad_request`, `not_found`, `rejected`, ...) is a
result with `isError: true` and the text `<code>: <message>`; an unknown tool is the JSON-RPC error `-32602`.
`idempotency_key` works as the API's `Idempotency-Key`: the same key and
arguments return the first result, other arguments are `idempotency_key_reused`.

## Protocol epochs

Both epochs are served, chosen per request:

- `MCP-Protocol-Version: 2026-07-28` — stateless: `server/discover`, `tools/list`, `tools/call`; results carry
  `resultType: "complete"`.
- Otherwise the `initialize` epoch: `initialize` answers the requested version if it is one of 2025-11-25,
  2025-06-18, 2025-03-26 (else 2025-11-25), `notifications/initialized` gets 202. No `Mcp-Session-Id` is issued,
  so a daemon restart never makes a client's session unknown. `server/discover` here is `-32601`, the answer on
  which claude falls back to `initialize`.

`GET /mcp` is 405: the server opens no stream of its own. One JSON-RPC message per request (no batches).

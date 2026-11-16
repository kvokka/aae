# API sweep (cycle 103)

## Routes vs openapi.json: match
Router crates/aae-front/src/api/mod.rs:86-114 (utoipa OpenApiRouter) = 29 operations on 27 paths, same set as
docs/api/openapi.json `.paths`. `/mcp` (POST,GET) is added by hand at mod.rs:164 and deliberately not in OpenAPI
(comment mod.rs:162). crates/aae/src/bridge.rs:117 is the stdio->HTTP mcp-bridge, not the API. Test
crates/aae-front/tests/openapi.rs checks the committed json against the code.

## D1 (real defect): query parameters are declared `in: path, required: true`
Every `#[derive(IntoParams)]` struct used with `Query<>` lacks `#[into_params(parameter_in = Query)]`
(utoipa default is Path). Result in docs/api/openapi.json: `project`/`state`/`parent`/`limit` of GET /v1/runs,
`project` of GET /v1/{projects,agents,formulas,packs,plugins}, `project/agent/formula/rig` of GET /v1/config/explain,
`from` of GET /v1/events (and likely LiveQuery `from`), `timeout_s` of POST /v1/daemon/{drain,restart} are all
"in": "path", "required": true, though they are optional query params (Option<..> in the structs).
Code: api/config.rs:19 (ProjectQuery), config.rs:149 (ExplainQuery), runs.rs:87 (ListQuery), events.rs:28,
live.rs:30, daemon.rs:42 (Drain). Fix: add `#[into_params(parameter_in = Query)]` to each, regenerate with
`AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`. (Generated clients would break on this.)

## Field names: match
- Formula (replies.rs:314-323): project, formula, description, inputs{type,required}, internal = openapi Formula.
- GET /v1/formulas has no `all` query; `--all` is client-side (cli/config.rs:27,79-87 retains internal != true).
  frontends.md:58-59 says exactly that. OK. MCP list_formulas filters internal on the server (mcp.rs:133-136).
- CreateRun (runs.rs:27-41): project, rig, formula, inputs, continue_from, parent{run,token} = openapi.
- GET /v1/runs query (runs.rs:87-95): project, state, parent, limit = openapi names (only `in` is wrong, D1).

## API <-> CLI table
There is no markdown table in docs/api/frontends.md or docs/guide; the correspondence is the const
`OPERATIONS` (cli/mod.rs:20-50, 29 rows) checked by crates/aae-front/tests/openapi.rs:36 against the OpenAPI
operations and (mod.rs:183) against the clap commands. 29 rows = 29 operations: no extras, none missing.
frontends.md prose: `aae formula list --all`, `run list --parent`, `run start`, `run watch/wait`, `finish`,
`config check|explain`, `cache clear`, `doctor` all exist in clap (commands.rs, config.rs, mod.rs:76+).
Not in OPERATIONS, correctly: `daemon`, `cache clear`, `systemd/launchd`, `run wait` (client-side over events),
hidden `mcp-bridge`.
Minor: frontends.md:43-44 says "One command per API operation" - true; `run wait` has no operation, not mentioned
as an exception (cosmetic).

## MCP tools: match
docs/api/mcp.md:13-19 (5 tools) = mcp.rs:31-53 (list_formulas, start_run, run_state, list_runs, cancel_run);
arguments equal (start_run: project, rig, formula, inputs, continue_from, idempotency_key; list_runs: project,
state, limit; cancel_run: id, reason, idempotency_key). Note: MCP list_runs has no `parent` (API/CLI do); doc
lists exactly what the tool has, so no discrepancy.

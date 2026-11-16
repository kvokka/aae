//! Runs: start a formula, list, state, cancel, answer a request, the finisher (spec §12, contracts 6).

use super::{
    ApiError, Body, ErrorReply, Param, Reply, State, key,
    replies::{Failed, Finished, Keyed, Run, Started},
};
use aae_core::run::{Create, Msg, Outcome, RunFilter, answer_from};
use axum::{
    Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use utoipa::{IntoParams, ToSchema};

/// Descriptions the operations share.
pub(super) const REJECTED: &str = "`rejected`: the core refused the request; the message says why";
pub(super) const NULL: &str = "Done; the body is JSON `null`";
pub(super) const BAD_QUERY: &str = "`bad_request`: the query is malformed";
const BAD_PATH: &str = "`bad_request`: the path is malformed";
const RUN_VIEW: &str = "The run: `RunView` (docs/design/contracts.md §1.3) with `project`, `rig`, `correlation`, \
    `inputs`, `waiting_for`, `outcome` and `attempts`";
const RUN_VIEWS: &str = "Run views as `GET /v1/runs/{id}` gives them";

#[derive(Deserialize, ToSchema)]
pub(crate) struct CreateRun {
    project: String,
    /// Absent: the rig of the id prefix of the bead its task names; else the run
    /// works in the project root.
    rig: Option<String>,
    /// `name` or `<pack>.<name>`, as the project writes it.
    formula: String,
    #[serde(default)]
    inputs: Map<String, Value>,
    /// A terminal run whose last session this run continues (spec §9.3).
    continue_from: Option<String>,
    /// The agent session that starts this run (`AAE_RUN`, `AAE_ATTEMPT_TOKEN`): the run is its child, with its
    /// correlation, one level deeper (docs/design/contracts.md 6.1). Absent: a new correlation at depth 0.
    parent: Option<ParentAttempt>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ParentAttempt {
    run: String,
    /// The attempt token of the session; a stale one is rejected.
    token: String,
}

/// Starts a run of a formula; the reply is `{"run": "<id>"}`, or `{"nothing_ready": true}` when a task input
/// `{ next_ready = true }` finds no ready bead to take (spec §10.3). With `parent`, a stale token or a chain deeper
/// than 10 is `rejected`.
#[utoipa::path(post, path = "/v1/runs", request_body = CreateRun,
    params(("Idempotency-Key" = Option<String>, Header, description = "Repeats return the first reply")),
    responses((status = OK, body = Started), (status = CONFLICT, description = REJECTED, body = ErrorReply), Keyed))]
pub(crate) async fn create(state: State, headers: HeaderMap, Body(body): Body<CreateRun>) -> Reply {
    let create = Create {
        project: body.project,
        rig: body.rig,
        formula: body.formula,
        inputs: body.inputs,
        correlation: None,
        depth: 0,
        token: body.parent.as_ref().map(|p| p.token.clone()),
        parent_run: body.parent.map(|p| p.run),
        continue_from: body.continue_from,
    };
    Ok(Json(state.core.send_request(key(&headers), Msg::Create(create)).await?))
}

/// The states of a run (docs/design/contracts.md 6.2).
const STATES: [&str; 11] = [
    "queued",
    "preparing",
    "starting",
    "running",
    "requires_action",
    "finishing",
    "interrupted",
    "parked",
    "passed",
    "failed",
    "cancelled",
];

#[derive(Deserialize, IntoParams)]
pub(crate) struct ListQuery {
    project: Option<String>,
    /// One of the run states; another is a bad request.
    state: Option<String>,
    /// The runs whose `parent_run` this is: those its sessions started and those its events triggered.
    parent: Option<String>,
    /// At most this many, newest first (default 100).
    limit: Option<u32>,
}

/// Lists runs, newest first.
#[utoipa::path(get, path = "/v1/runs", params(ListQuery),
    responses((status = OK, description = RUN_VIEWS, body = [Run]), (status = BAD_REQUEST, description = BAD_QUERY,
        body = ErrorReply), Failed))]
pub(crate) async fn list(state: State, Param(Query(query)): Param<Query<ListQuery>>) -> Reply {
    if let Some(s) = query.state.as_deref().filter(|s| !STATES.contains(s)) {
        let why = format!("no run state `{s}`; one of {}", STATES.join(", "));
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "bad_request", why));
    }
    let filter = RunFilter {
        project: query.project,
        state: query.state,
        parent: query.parent,
        limit: query.limit,
    };
    Ok(Json(state.reader.runs(&filter)?.into()))
}

/// The state of a run: its view, where it waits and its attempts.
#[utoipa::path(get, path = "/v1/runs/{id}", params(("id" = String, Path)),
    responses((status = OK, description = RUN_VIEW, body = Run), (status = BAD_REQUEST, description = BAD_PATH,
        body = ErrorReply), (status = NOT_FOUND, description = "`not_found`: no such run", body = ErrorReply), Failed))]
pub(crate) async fn get(state: State, Param(Path(id)): Param<Path<String>>) -> Reply {
    let run = state.reader.run(&id)?;
    run.map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("no run `{id}`")))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CancelRun {
    #[serde(default)]
    reason: String,
}

/// Cancels a run.
#[utoipa::path(post, path = "/v1/runs/{id}/cancel", request_body = CancelRun,
    params(("id" = String, Path), ("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, description = NULL, body = ()),
        (status = CONFLICT, description = REJECTED, body = ErrorReply),
        Keyed))]
pub(crate) async fn cancel(
    state: State,
    headers: HeaderMap,
    Param(Path(run)): Param<Path<String>>,
    Body(body): Body<CancelRun>,
) -> Reply {
    let reason = Some(body.reason)
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "by the operator".into());
    Ok(Json(
        state
            .core
            .send_request(key(&headers), Msg::Cancel { run, reason })
            .await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AnswerRun {
    /// The id of the open request.
    request: String,
    /// `{"decision": "allow", "updated_input"?}`, `{"decision": "deny", "message"}` or
    /// `{"decision": "answer", "text"}`.
    answer: Value,
}

/// Answers the open request of a run in `requires_action`.
#[utoipa::path(post, path = "/v1/runs/{id}/answer", request_body = AnswerRun,
    params(("id" = String, Path), ("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, description = NULL, body = ()),
        (status = CONFLICT, description = REJECTED, body = ErrorReply),
        Keyed))]
pub(crate) async fn answer(
    state: State,
    headers: HeaderMap,
    Param(Path(run)): Param<Path<String>>,
    Body(body): Body<AnswerRun>,
) -> Reply {
    let answer = answer_from(&body.answer)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", "not an answer"))?;
    let msg = Msg::Answer {
        run,
        request: body.request,
        answer,
    };
    Ok(Json(state.core.send_request(key(&headers), msg).await?))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct FinishRun {
    /// The attempt token the session got.
    token: String,
    /// `{"result": "pass"|"fail", "summary", "data"?}`.
    #[schema(value_type = Object)]
    outcome: Outcome,
}

/// The finisher: the agent declares the outcome of its attempt (contracts 6.3); the reply is
/// `{"answer": "accepted"|"stale_attempt"|"run_terminal"|"invalid"}`.
#[utoipa::path(post, path = "/v1/runs/{id}/finish", request_body = FinishRun,
    params(("id" = String, Path), ("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Finished), Keyed))]
pub(crate) async fn finish(
    state: State,
    headers: HeaderMap,
    Param(Path(run)): Param<Path<String>>,
    Body(body): Body<FinishRun>,
) -> Reply {
    let msg = Msg::Finish {
        run,
        token: body.token,
        outcome: body.outcome,
    };
    Ok(Json(state.core.send_request(key(&headers), msg).await?))
}

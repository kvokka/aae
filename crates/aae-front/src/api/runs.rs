//! Runs: start a formula, list, state, cancel, answer a request, the finisher (spec §12, contracts 6).

use super::{ApiError, Body, Reply, State, key};
use aae_core::run::{Create, Msg, Outcome, RunFilter, answer_from};
use axum::{
    Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use utoipa::{IntoParams, ToSchema};

#[derive(Deserialize, ToSchema)]
pub(crate) struct CreateRun {
    project: String,
    rig: String,
    /// `name` or `<pack>.<name>`, as the project writes it.
    formula: String,
    #[serde(default)]
    inputs: Map<String, Value>,
    /// A terminal run whose last session this run continues (spec §9.3).
    continue_from: Option<String>,
}

/// Starts a run of a formula; the reply is `{"run": "<id>"}`, or `{"nothing_ready": true}` when a bead input
/// `{ next_ready = true }` finds no ready bead to take (spec §10.3).
#[utoipa::path(post, path = "/v1/runs", request_body = CreateRun,
    params(("Idempotency-Key" = Option<String>, Header, description = "Repeats return the first reply")),
    responses((status = OK, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn create(state: State, headers: HeaderMap, Body(body): Body<CreateRun>) -> Reply {
    let create = Create {
        project: body.project,
        rig: body.rig,
        formula: body.formula,
        inputs: body.inputs,
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: body.continue_from,
    };
    Ok(Json(state.core.send_request(key(&headers), Msg::Create(create)).await?))
}

#[derive(Deserialize, IntoParams)]
pub(crate) struct ListQuery {
    project: Option<String>,
    state: Option<String>,
    /// At most this many, newest first (default 100).
    limit: Option<u32>,
}

/// Lists runs, newest first.
#[utoipa::path(get, path = "/v1/runs", params(ListQuery), responses((status = OK, body = [Object])))]
pub(crate) async fn list(state: State, Query(query): Query<ListQuery>) -> Reply {
    let filter = RunFilter {
        project: query.project,
        state: query.state,
        limit: query.limit,
    };
    Ok(Json(state.reader.runs(&filter)?.into()))
}

/// The state of a run: its view, where it waits and its attempts.
#[utoipa::path(get, path = "/v1/runs/{id}", params(("id" = String, Path)),
    responses((status = OK, body = Object), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn get(state: State, Path(id): Path<String>) -> Reply {
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
    responses((status = OK, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn cancel(
    state: State,
    headers: HeaderMap,
    Path(run): Path<String>,
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
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn answer(
    state: State,
    headers: HeaderMap,
    Path(run): Path<String>,
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
    responses((status = OK, body = Object)))]
pub(crate) async fn finish(
    state: State,
    headers: HeaderMap,
    Path(run): Path<String>,
    Body(body): Body<FinishRun>,
) -> Reply {
    let msg = Msg::Finish {
        run,
        token: body.token,
        outcome: body.outcome,
    };
    Ok(Json(state.core.send_request(key(&headers), msg).await?))
}

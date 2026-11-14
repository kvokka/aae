//! The instance registry (contracts 8): the list, and the operator's stop of an idle instance.

use super::{
    ErrorReply, Param, Reply, State, key,
    replies::{Failed, Instance, InstanceStopping, Keyed},
    runs::REJECTED,
};
use aae_core::run::Msg;
use axum::{Json, extract::Path, http::HeaderMap};
use serde_json::json;

/// Lists every harness instance: `{id, kind, harness, fingerprint, state, sessions, capacity, pid, pgid,
/// idle_since}`.
#[utoipa::path(get, path = "/v1/instances", responses((status = OK, body = [Instance]), Failed))]
pub(crate) async fn list(state: State) -> Reply {
    Ok(Json(state.reader.instances()?.into()))
}

/// Stops an idle instance (`instance.stopped{reason: "operator"}`); one that starts or has sessions is refused
/// (409), so no attempt loses its process.
#[utoipa::path(post, path = "/v1/instances/{id}/stop",
    params(("id" = String, Path), ("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = InstanceStopping), (status = CONFLICT, description = REJECTED, body = ErrorReply), Keyed))]
pub(crate) async fn stop(state: State, headers: HeaderMap, Param(Path(id)): Param<Path<String>>) -> Reply {
    state.core.send_request(key(&headers), Msg::StopInstance { id }).await?;
    Ok(Json(json!({ "stopping": true })))
}

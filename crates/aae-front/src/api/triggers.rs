//! Triggers by hand (spec §9.5): fire one on a rig as its schedule would.

use super::{Body, Reply, State, key};
use aae_core::run::Msg;
use axum::{Json, http::HeaderMap};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub(crate) struct FireTrigger {
    project: String,
    rig: String,
    /// `<pack>.<name>`, or the name of a project trigger.
    trigger: String,
}

/// Fires a trigger by hand: `{"run": id}`, or `{"skipped": reason, "detail": text}` when it does not start a run
/// (an overlapping live run, for one).
#[utoipa::path(post, path = "/v1/triggers/fire", request_body = FireTrigger,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn fire(state: State, headers: HeaderMap, Body(body): Body<FireTrigger>) -> Reply {
    let msg = Msg::Fire {
        project: body.project,
        rig: body.rig,
        trigger: body.trigger,
    };
    Ok(Json(state.core.send(key(&headers), msg).await?))
}

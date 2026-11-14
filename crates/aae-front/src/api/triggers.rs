//! Triggers (spec §9.5): the list with the next firings, firing one on a rig by hand as its schedule would, and
//! switching one off and on.

use super::{Body, Reply, State, key};
use aae_core::run::Msg;
use axum::{Json, http::HeaderMap};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub(crate) struct OnRig {
    project: String,
    rig: String,
    /// `<pack>.<name>`, or the name of a project trigger.
    trigger: String,
}

/// Fires a trigger by hand: `{"run": id}`, or `{"skipped": reason, "detail": text}` when it does not start a run
/// (an overlapping live run, for one).
#[utoipa::path(post, path = "/v1/triggers/fire", request_body = OnRig,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn fire(state: State, headers: HeaderMap, Body(body): Body<OnRig>) -> Reply {
    let msg = Msg::Fire {
        project: body.project,
        rig: body.rig,
        trigger: body.trigger,
    };
    Ok(Json(state.core.send_request(key(&headers), msg).await?))
}

/// Switches a trigger on a rig back on: `{"trigger": name, "enabled": true}`.
#[utoipa::path(post, path = "/v1/triggers/enable", request_body = OnRig,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn enable(state: State, headers: HeaderMap, Body(body): Body<OnRig>) -> Reply {
    switch(state, headers, body, true).await
}

/// Switches a trigger on a rig off: neither its schedule nor its events fire it until it is enabled, a hand firing
/// still does. `{"trigger": name, "enabled": false}`.
#[utoipa::path(post, path = "/v1/triggers/disable", request_body = OnRig,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn disable(state: State, headers: HeaderMap, Body(body): Body<OnRig>) -> Reply {
    switch(state, headers, body, false).await
}

async fn switch(state: State, headers: HeaderMap, body: OnRig, enabled: bool) -> Reply {
    let msg = Msg::SwitchTrigger {
        project: body.project,
        rig: body.rig,
        trigger: body.trigger,
        enabled,
    };
    Ok(Json(state.core.send_request(key(&headers), msg).await?))
}

/// Lists every trigger on every rig it runs on, with its source and next firing (`next_at`, null for an event
/// trigger) and whether it is `enabled`.
#[utoipa::path(get, path = "/v1/triggers", responses((status = OK, body = [Object])))]
pub(crate) async fn list(state: State) -> Reply {
    Ok(Json(state.reader.triggers(&state.config())?.into()))
}

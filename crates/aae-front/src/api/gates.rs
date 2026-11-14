//! Issue gates by hand (spec §5.7, contracts 7): list, close as the operator, open.

use super::{ApiError, Body, Reply, State, key};
use aae_core::{
    parse_rfc3339_ms,
    run::{GateSpec, Msg, valid_key},
};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use utoipa::ToSchema;

/// Every active closure, oldest first.
#[utoipa::path(get, path = "/v1/gates", responses((status = OK, body = [Object])))]
pub(crate) async fn list(state: State) -> Reply {
    Ok(Json(state.reader.gates(now_ms())?.into()))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CloseGate {
    /// `orchestrator`, `project:<p>`, `rig:<p>/<r>`, `agent:<a>`, `bead:<p>/<id>`, `account:<h>/<id>`,
    /// `model:<provider>/<model>`.
    key: String,
    /// `YYYY-MM-DDTHH:MM:SS[.fff]Z`; absent: until opened by hand.
    until: Option<String>,
    #[serde(default)]
    reason: String,
}

/// Closes a gate as the operator: automation never opens it (criterion 19).
#[utoipa::path(post, path = "/v1/gates", request_body = CloseGate,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn close(state: State, headers: HeaderMap, Body(body): Body<CloseGate>) -> Reply {
    valid(&body.key)?;
    let until_ms = match &body.until {
        Some(until) => Some(parse_rfc3339_ms(until).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                format!("`{until}` is not a UTC time"),
            )
        })?),
        None => None,
    };
    let gate = GateSpec {
        key: body.key,
        until_ms,
        reason: body.reason,
    };
    Ok(Json(
        state.core.send_request(key(&headers), Msg::CloseGate(gate)).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct OpenGate {
    key: String,
}

/// Opens a gate: every closure of the key goes, the policy's too.
#[utoipa::path(post, path = "/v1/gates/open", request_body = OpenGate,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn open(state: State, headers: HeaderMap, Body(body): Body<OpenGate>) -> Reply {
    valid(&body.key)?;
    let msg = Msg::OpenGate { key: body.key };
    Ok(Json(state.core.send_request(key(&headers), msg).await?))
}

/// A malformed key is the request's fault (400); a well-formed one the core may still refuse (409).
fn valid(key: &str) -> Result<(), ApiError> {
    let message = || format!("`{key}` is not a gate key");
    valid_key(key)
        .then_some(())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", message()))
}

/// The wall clock, as the daemon's system clock reads it.
fn now_ms() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

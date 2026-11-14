//! The event stream (spec §5.3): the journal as SSE from a `seq`, then live. A `seq` rotated away is the explicit
//! "gap" error; the client recovers by reading the state.

use super::{ApiError, ErrorReply, Param, State, replies::Failed, runs::BAD_QUERY};
use aae_core::ReadError;
use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use utoipa::IntoParams;

/// The 410 of an event stream.
pub(super) const GAP: &str = "`gap`: the `seq` was rotated away; `oldest` is the first one still held";

/// Events read from the journal at a time.
const BATCH: usize = 256;

#[derive(Deserialize, IntoParams)]
pub(crate) struct StreamQuery {
    /// The first `seq` to send; absent: the events after the last written one (or after `Last-Event-ID`).
    from: Option<i64>,
}

/// The events as SSE: `id` is the `seq`, `event` the type, `data` the envelope (contracts 1.1). A `seq` rotated
/// away is `410` with `oldest`, the first `seq` still held; it rotated away while streaming: an event `gap`.
#[utoipa::path(get, path = "/v1/events", params(StreamQuery, ("Last-Event-ID" = Option<String>, Header)),
    responses((status = OK, content_type = "text/event-stream", body = String),
        (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply), (status = GONE, description = GAP, body = ErrorReply),
        Failed))]
pub(crate) async fn stream(
    state: State,
    headers: HeaderMap,
    Param(Query(query)): Param<Query<StreamQuery>>,
) -> Result<Response, ApiError> {
    let from = query
        .from
        .or_else(|| after_last(&headers))
        .unwrap_or_else(|| *state.written.borrow() + 1);
    // The gap is reported before the stream starts, while a status code can still say it.
    read(&state, from, 1).map_err(|e| gap(&e))?;
    let (tx, rx) = mpsc::channel(BATCH);
    let state = state.0.clone();
    tokio::spawn(async move {
        let mut written = state.written.clone();
        let mut next = from;
        loop {
            // Marked seen before the read: a write after it wakes the wait below.
            written.borrow_and_update();
            match read(&state, next, BATCH) {
                Ok(events) if events.is_empty() => {
                    if written.changed().await.is_err() {
                        return;
                    }
                }
                Ok(events) => {
                    for envelope in events {
                        next = envelope["seq"].as_i64().unwrap_or(next) + 1;
                        if tx.send(Ok(event(&envelope))).await.is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    let data = gap(&e).message;
                    let _ = tx.send(Ok(Event::default().event("gap").data(data))).await;
                    return;
                }
            }
        }
    });
    let stream: ReceiverStream<Result<Event, Infallible>> = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}

/// An envelope as an SSE event: `id` the `seq`, `event` the type.
pub(super) fn event(envelope: &Value) -> Event {
    Event::default()
        .id(envelope["seq"].to_string())
        .event(envelope["type"].as_str().unwrap_or("event"))
        .data(envelope.to_string())
}

/// The `seq` after the `Last-Event-ID` of a reconnecting client.
pub(super) fn after_last(headers: &HeaderMap) -> Option<i64> {
    headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok()?.parse::<i64>().ok())
        .map(|id| id + 1)
}

// shortcut: every read scans the journal files from the oldest; index the files by first `seq` if streams lag.
pub(super) fn read(state: &super::Api, from: i64, limit: usize) -> Result<Vec<Value>, ReadError> {
    state.journal.read_from(from, limit)
}

pub(super) fn gap(e: &ReadError) -> ApiError {
    match e {
        ReadError::Gap { oldest } => {
            let mut error = ApiError::new(
                StatusCode::GONE,
                "gap",
                format!("the events before {oldest} were rotated away; read the state and stream from {oldest}"),
            );
            error.extra = json!({ "oldest": oldest });
            error
        }
        ReadError::Io(e) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "journal", e.to_string()),
    }
}

//! The live view of a run (spec §5.3, §12): its events from the journal as SSE from a `seq`, and the raw messages
//! of its current session relayed from the adapter as they come. The core never stores those messages, so a
//! reconnect gets only the new ones; the history is in the harness's transcript.

use super::{
    Api, ApiError, State,
    events::{after_last, event, gap, read},
};
use aae_core::{ReadError, driver::SessionRef};
use axum::{
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use utoipa::IntoParams;

/// Events read from the journal at a time.
const BATCH: usize = 256;
/// How often the view looks for a new session of the run while nothing else happens.
const LOOK: Duration = Duration::from_millis(500);

#[derive(Deserialize, IntoParams)]
pub(crate) struct LiveQuery {
    /// The first `seq` to send; absent: from the oldest event held (or after `Last-Event-ID`).
    from: Option<i64>,
}

/// The live view of a run as SSE: its events as on `/v1/events` (`id` the `seq`); the raw messages of its current
/// session as `event: harness` with no `id`; `event: lagged` with `{"missed": n}` when the reader fell behind the
/// session. The stream ends after the run's `run.passed`, `run.failed` or `run.cancelled`.
#[utoipa::path(get, path = "/v1/runs/{id}/live",
    params(("id" = String, Path), LiveQuery, ("Last-Event-ID" = Option<String>, Header)),
    responses((status = OK, content_type = "text/event-stream", body = String), (status = NOT_FOUND, body = Object),
        (status = GONE, body = Object)))]
pub(crate) async fn live(
    state: State,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<LiveQuery>,
) -> Result<Response, ApiError> {
    if state.reader.run(&id)?.is_none() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("no run `{id}`"),
        ));
    }
    let from = match query.from.or_else(|| after_last(&headers)) {
        Some(from) => {
            read(&state, from, 1).map_err(|e| gap(&e))?;
            from
        }
        None => match read(&state, 1, 1) {
            Err(ReadError::Gap { oldest }) => oldest,
            Err(e) => return Err(gap(&e)),
            Ok(_) => 1,
        },
    };
    let (tx, rx) = mpsc::channel(BATCH);
    // shortcut: a terminal run whose end rotated out of the journal keeps the stream open until the client leaves;
    // record the seq of a run's end in the state if a client needs that.
    tokio::spawn(relay(state.0.clone(), id, from, tx));
    let stream: ReceiverStream<Result<Event, Infallible>> = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}

async fn relay(state: Arc<Api>, run: String, mut next: i64, tx: mpsc::Sender<Result<Event, Infallible>>) {
    let mut written = state.written.clone();
    let (mut session, mut tap) = (Value::Null, None);
    let mut caught_up = false;
    loop {
        if caught_up {
            tokio::select! {
                changed = written.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    caught_up = false;
                }
                message = recv(&mut tap) => {
                    let event = match message {
                        Ok(message) => Event::default().event("harness").data(message.to_string()),
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            Event::default().event("lagged").data(json!({ "missed": n }).to_string())
                        }
                        // The session ended; a later attempt's session is looked up below.
                        Err(broadcast::error::RecvError::Closed) => {
                            tap = None;
                            continue;
                        }
                    };
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                    continue;
                }
                () = tokio::time::sleep(LOOK) => {}
            }
            look(&state, &run, &mut session, &mut tap);
            continue;
        }
        // Marked seen before the read: a write after it wakes the wait above.
        written.borrow_and_update();
        match read(&state, next, BATCH) {
            Ok(events) if events.is_empty() => {
                caught_up = true;
                look(&state, &run, &mut session, &mut tap);
            }
            Ok(events) => {
                for envelope in events {
                    next = envelope["seq"].as_i64().unwrap_or(next) + 1;
                    if envelope["run"] != run.as_str() {
                        continue;
                    }
                    if tx.send(Ok(event(&envelope))).await.is_err() {
                        return;
                    }
                    if matches!(
                        envelope["type"].as_str(),
                        Some("run.passed" | "run.failed" | "run.cancelled")
                    ) {
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
}

/// Taps the session of the run's last attempt if it is not the one tapped already.
fn look(state: &Api, run: &str, session: &mut Value, tap: &mut Option<broadcast::Receiver<Value>>) {
    let Ok(Some(view)) = state.reader.run(run) else {
        return;
    };
    let Some(last) = view["attempts"].as_array().and_then(|a| a.last()) else {
        return;
    };
    if last["session"].is_null() || last["session"] == *session {
        return;
    }
    let driver = state.drivers.iter().find(|d| last["harness"] == d.harness());
    // Taken as tapped only once the driver has its stream, so a session not yet known to it is looked up again.
    if let Some(receiver) = driver.and_then(|d| d.live(&SessionRef(last["session"].clone()))) {
        *tap = Some(receiver);
        session.clone_from(&last["session"]);
    }
}

async fn recv(tap: &mut Option<broadcast::Receiver<Value>>) -> Result<Value, broadcast::error::RecvError> {
    match tap {
        Some(tap) => tap.recv().await,
        None => std::future::pending().await,
    }
}

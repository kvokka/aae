//! The live view of a run (spec §5.3, §12): its events from the journal as SSE from a `seq`, and the raw messages
//! of its current session relayed from the adapter as they come. The core never stores those messages, so a
//! reconnect gets only the new ones; the history is in the harness's transcript.

use super::{
    Api, ApiError, ErrorReply, Param, State,
    events::{GAP, after_last, event, gap, read},
    replies::Failed,
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
    responses((status = OK, content_type = "text/event-stream", body = String),
        (status = BAD_REQUEST, description = "`bad_request`: the path or query is malformed", body = ErrorReply),
        (status = NOT_FOUND, description = "`not_found`: no such run", body = ErrorReply),
        (status = GONE, description = GAP, body = ErrorReply), Failed))]
pub(crate) async fn live(
    state: State,
    Param(Path(id)): Param<Path<String>>,
    headers: HeaderMap,
    Param(Query(query)): Param<Query<LiveQuery>>,
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
    tokio::spawn(relay(state.0.clone(), id, from, tx));
    let stream: ReceiverStream<Result<Event, Infallible>> = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}

async fn relay(state: Arc<Api>, run: String, mut next: i64, tx: mpsc::Sender<Result<Event, Infallible>>) {
    let mut written = state.written.clone();
    let (mut session, mut tap) = (Value::Null, None);
    let mut caught_up = false;
    // A seq at or after the run's end, once the state shows the run ended: the view closes when it has read past it,
    // also when the end is before `from` or rotated out of the journal.
    let mut end = None;
    loop {
        if caught_up {
            if end.is_some_and(|end| next > end) {
                return;
            }
            tokio::select! {
                () = tx.closed() => return,
                changed = written.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    caught_up = false;
                }
                event = tapped(&mut tap) => {
                    // `None`: the session ended; a later attempt's session is looked up below.
                    let Some(event) = event else { continue };
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                    continue;
                }
                () = tokio::time::sleep(LOOK) => {}
            }
            look(&state, &run, &mut session, &mut tap, &mut end);
            continue;
        }
        // Marked seen before the read: a write after it wakes the wait above.
        written.borrow_and_update();
        match read(&state, next, BATCH) {
            Ok(events) if events.is_empty() => {
                caught_up = true;
                look(&state, &run, &mut session, &mut tap, &mut end);
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

/// Taps the session of the run's last attempt if it is not the one tapped already; sets `end` once the run ended.
fn look(
    state: &Api,
    run: &str,
    session: &mut Value,
    tap: &mut Option<broadcast::Receiver<Value>>,
    end: &mut Option<i64>,
) {
    let Ok(Some(view)) = state.reader.run(run) else {
        return;
    };
    if end.is_none() && matches!(view["state"].as_str(), Some("passed" | "failed" | "cancelled")) {
        // The run's last event was committed before this read, so its seq is at most the last one assigned.
        *end = state.reader.last_seq().ok();
        return;
    }
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

/// The next message of the tapped session as an event: `harness`, or `lagged` with `{"missed": n}` when the reader
/// fell behind; `None` once the session ended, and the tap is dropped then. Pending while nothing is tapped.
async fn tapped(tap: &mut Option<broadcast::Receiver<Value>>) -> Option<Event> {
    let Some(receiver) = tap else {
        return std::future::pending().await;
    };
    match receiver.recv().await {
        Ok(message) => Some(Event::default().event("harness").data(message.to_string())),
        Err(broadcast::error::RecvError::Lagged(n)) => Some(
            Event::default()
                .event("lagged")
                .data(json!({ "missed": n }).to_string()),
        ),
        Err(broadcast::error::RecvError::Closed) => {
            *tap = None;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn a_reader_behind_the_session_gets_lagged_with_the_missed_count_then_the_rest() {
        let (sender, receiver) = broadcast::channel(2);
        for n in 0..5 {
            sender.send(json!({ "n": n })).unwrap();
        }
        drop(sender);
        let mut tap = Some(receiver);
        let mut events = Vec::new();
        while let Some(event) = tapped(&mut tap).await {
            events.push(Ok::<_, Infallible>(event));
        }
        assert!(tap.is_none(), "the ended session is no longer tapped");
        let body = Sse::new(tokio_stream::iter(events)).into_response().into_body();
        let text = String::from_utf8(body.collect().await.unwrap().to_bytes().to_vec()).unwrap();
        assert_eq!(
            text,
            "event: lagged\ndata: {\"missed\":3}\n\n\
             event: harness\ndata: {\"n\":3}\n\n\
             event: harness\ndata: {\"n\":4}\n\n"
        );
    }
}

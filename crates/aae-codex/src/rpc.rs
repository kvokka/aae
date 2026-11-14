//! JSON-RPC with app-server over its stdio: requests and their answers, server requests, and the reader that turns
//! the rest into events.

use crate::stream::Stream;
use aae_core::driver::{
    DriverEvent, EventSink, Failure, FailureKind, Normalized, PromptKey, SessionRef, Source, Transcript,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::ChildStdin,
    sync::{broadcast, oneshot},
};

/// How long a request to app-server may go unanswered before the process counts as hung.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(60);

type Reply = Result<Value, Value>;

pub(crate) struct Instance {
    pub(crate) home: Value,
    pub(crate) stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    pub(crate) next: AtomicU64,
    pub(crate) pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    /// The `turn/start` requests in flight: their answer acknowledges the prompt of the thread.
    pub(crate) acks: Mutex<HashMap<u64, (String, PromptKey)>>,
    pub(crate) stream: Mutex<Stream>,
    /// The open server requests, whole, by the id the core knows.
    pub(crate) requests: Mutex<HashMap<String, Value>>,
    /// The turn in flight of each thread, for `turn/interrupt`.
    pub(crate) turns: Mutex<HashMap<String, String>>,
    pub(crate) live: Mutex<HashMap<String, broadcast::Sender<Value>>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn crashed() -> Failure {
    Failure::new(FailureKind::HarnessCrash, Source::Adapter)
}

/// The transcript a failed `thread/read` shows: an answer that the thread cannot be read means it is missing; no
/// answer (a stall, a crash) says nothing about it, so the attempt fails and the session is kept.
pub(crate) fn unread(failure: Failure) -> Result<Transcript, Failure> {
    match failure.kind {
        FailureKind::Stall | FailureKind::HarnessCrash => Err(failure),
        _ => Ok(Transcript::Missing),
    }
}

impl Instance {
    pub(crate) async fn write(&self, msg: &Value) -> Result<(), Failure> {
        let mut stdin = self.stdin.lock().await;
        let stdin = stdin.as_mut().ok_or_else(crashed)?;
        stdin
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .map_err(|_| crashed())?;
        stdin.flush().await.map_err(|_| crashed())
    }

    /// One request and its answer; an error answer is a failure of the attempt.
    pub(crate) async fn call(
        &self,
        method: &str,
        params: Value,
        ack: Option<(String, PromptKey)>,
    ) -> Result<Value, Failure> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        if let Some(ack) = ack {
            lock(&self.acks).insert(id, ack);
        }
        self.write(&json!({ "id": id, "method": method, "params": params }))
            .await?;
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(rpc_failure(method, &error)),
            Ok(Err(_)) => Err(crashed()),
            Err(_) => {
                let mut failure = Failure::new(FailureKind::Stall, Source::Adapter);
                failure.raw = format!("`{method}` got no answer in {}s", CALL_TIMEOUT.as_secs());
                Err(failure)
            }
        }
    }
}

/// An error answer: a second writer or a lost rollout is a transcript divergence (contracts 4.6), the rest unknown.
fn rpc_failure(method: &str, error: &Value) -> Failure {
    let text = error["message"].as_str().unwrap_or_default();
    let kind = match text {
        t if t.contains("already has an active writer") || t.contains("no rollout found") => {
            FailureKind::TranscriptDivergence
        }
        _ if error["data"]["type"] == "modelProviderMismatch" => FailureKind::ConfigError,
        _ => FailureKind::Unknown,
    };
    let mut failure = Failure::new(kind, Source::Adapter);
    failure.text = (kind == FailureKind::Unknown).then(|| text.to_owned());
    failure.raw = format!("{method}: {error}");
    failure
}

/// The reference of a thread of the process with `home`; events carry the same one the core stored.
pub(crate) fn session(thread: &str, home: &Value) -> SessionRef {
    SessionRef(json!({ "thread_id": thread, "codex_home": home }))
}

/// The event sender of an instance: `seq` counts from 1.
pub(crate) fn sender(instance: String, home: Value, sink: EventSink) -> impl FnMut(Option<String>, Normalized) {
    let mut seq = 0;
    move |thread, event| {
        seq += 1;
        let session = thread.map(|t| session(&t, &home));
        let _ = sink.send(DriverEvent {
            instance: instance.clone(),
            session,
            seq,
            event,
        });
    }
}

/// Reads app-server's stdout to its end: answers go to their callers, the rest through the stream.
pub(crate) async fn read(
    instance: &Instance,
    mut lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    send: &mut impl FnMut(Option<String>, Normalized),
) {
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(method) = msg["method"].as_str() else {
            let Some(id) = msg["id"].as_u64() else { continue };
            let reply = match msg.get("error") {
                Some(error) => Err(error.clone()),
                None => Ok(msg["result"].clone()),
            };
            if let (Some((thread, key)), Ok(result)) = (lock(&instance.acks).remove(&id), &reply) {
                let turn = result["turn"]["id"].as_str().unwrap_or_default().to_owned();
                lock(&instance.turns).insert(thread.clone(), turn);
                lock(&instance.stream).accepted(&thread);
                send(Some(thread), Normalized::PromptAcked { key });
            }
            if let Some(tx) = lock(&instance.pending).remove(&id) {
                let _ = tx.send(reply);
            }
            continue;
        };
        let thread = msg["params"]["threadId"].as_str().unwrap_or_default();
        if let Some(live) = lock(&instance.live).get(thread) {
            let _ = live.send(msg.clone());
        }
        if method == "turn/started" {
            let turn = msg["params"]["turn"]["id"].as_str().unwrap_or_default().to_owned();
            lock(&instance.turns).insert(thread.to_owned(), turn);
        } else if method == "turn/completed" {
            lock(&instance.turns).remove(thread);
        }
        let events = lock(&instance.stream).message(&msg, Instant::now());
        if msg.get("id").is_some() {
            let asks = events
                .iter()
                .any(|(_, e)| matches!(e, Normalized::PermissionRequested { .. }));
            if asks {
                lock(&instance.requests).insert(msg["id"].to_string(), msg.clone());
            } else {
                // A server request the adapter has no answer for would hold the turn forever.
                let error = json!({ "code": -32601, "message": format!("aae does not answer `{method}`") });
                let _ = instance.write(&json!({ "id": msg["id"], "error": error })).await;
            }
        }
        for (thread, event) in events {
            send(thread, event);
        }
    }
}

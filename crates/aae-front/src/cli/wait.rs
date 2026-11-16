//! `aae run wait`: the outcomes of runs, waited for on the event stream (docs/overview.md item 2), not by polling.

use super::{print, run_path};
use crate::client::{Client, ClientError};
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::ExitCode, time::Duration};
use tokio::time::Instant;

/// The exit code when `--timeout` ran out first, as `timeout(1)` has it.
const EX_TIMEOUT: u8 = 124;

/// Waits until every run of `ids` is terminal, then prints each: exit 0 when all passed, 1 when not, 124 when
/// `timeout` ran out first (what is known by then is printed). A lost daemon is reconnected to until then.
pub(super) async fn wait(
    client: &Client,
    json: bool,
    ids: Vec<String>,
    timeout: Option<Duration>,
) -> Result<ExitCode, ClientError> {
    let deadline = timeout.map(|t| Instant::now() + t);
    let mut ended: BTreeMap<String, Value> = BTreeMap::new();
    let mut pause = Duration::from_millis(250);
    let waited = async {
        loop {
            match until_ended(client, &ids, &mut ended).await {
                Ok(()) => return Ok(()),
                // A restarting daemon: reconnect, then read the states again for what the stream missed.
                Err(ClientError::Io(_)) | Err(ClientError::Api { status: 503, .. }) => {
                    tokio::time::sleep(pause).await;
                    pause = (pause * 2).min(Duration::from_secs(5));
                }
                Err(e) => return Err(e),
            }
        }
    };
    let timed_out = match deadline {
        Some(at) => tokio::time::timeout_at(at, waited).await.is_err(),
        None => waited.await.map(|()| false)?,
    };
    let mut views = Vec::new();
    for id in &ids {
        let view = match ended.get(id) {
            Some(view) => view.clone(),
            // Out of time: the state as it is now, if the daemon answers.
            None => client
                .request("GET", &run_path(id, ""), None, None)
                .await
                .unwrap_or_default(),
        };
        views.push(outcome(id, &view));
    }
    print(json, &Value::Array(views.clone()));
    Ok(if timed_out {
        ExitCode::from(EX_TIMEOUT)
    } else if views.iter().all(|v| v["state"] == "passed") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// One connection: the stream opens before the states are read, so an end between the two is in the stream.
async fn until_ended(client: &Client, ids: &[String], ended: &mut BTreeMap<String, Value>) -> Result<(), ClientError> {
    let mut events = client.events(None).await?;
    for id in ids {
        check(client, id, ended).await?;
    }
    while ids.iter().any(|id| !ended.contains_key(id)) {
        let Some(event) = events.next().await? else {
            return Err(ClientError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        };
        let envelope: Value = serde_json::from_str(&event.data).unwrap_or_default();
        let run = envelope["run"].as_str().unwrap_or_default();
        let terminal = ["run.passed", "run.failed", "run.cancelled"].contains(&event.event.as_str());
        if terminal && ids.iter().any(|id| id == run) {
            check(client, run, ended).await?;
        }
        // A `gap` or a failed journal read ends the stream; the states are read again on the next connection.
        if event.id.is_none() {
            return Err(ClientError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
    }
    Ok(())
}

/// Reads the run and keeps its view when it is terminal; an unknown run is the API's `not_found`.
async fn check(client: &Client, id: &str, ended: &mut BTreeMap<String, Value>) -> Result<(), ClientError> {
    if ended.contains_key(id) {
        return Ok(());
    }
    let view = client.request("GET", &run_path(id, ""), None, None).await?;
    if matches!(view["state"].as_str(), Some("passed" | "failed" | "cancelled")) {
        ended.insert(id.to_owned(), view);
    }
    Ok(())
}

/// What the wait tells of a run: its state, why it failed, and what its finisher declared.
fn outcome(id: &str, view: &Value) -> Value {
    json!({
        "id": id, "state": view["state"], "class": view["class"], "reason": view["reason"],
        "summary": view["outcome"]["summary"], "data": view["outcome"]["data"],
    })
}

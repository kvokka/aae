//! The finisher (contracts 6.3): `loop-manager finish pass|fail` inside an agent session.

use super::{Verdict, fail, print, run_path};
use crate::client::{Client, ClientError};
use serde_json::{Value, json};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    process::ExitCode,
    time::Duration,
};

/// The finisher gives up on an unreachable daemon after this long and exits with `EX_TEMPFAIL` (contracts 6.3);
/// `LOOP_MANAGER_FINISH_RETRY` (seconds) overrides it.
const FINISH_RETRY: Duration = Duration::from_secs(60);
const EX_TEMPFAIL: u8 = 75;

/// The finisher: the run and the token come from the session environment; an unreachable daemon is retried.
pub(super) async fn finish(
    client: &Client,
    json: bool,
    result: Verdict,
    summary: String,
    data: Option<String>,
) -> Result<ExitCode, ClientError> {
    let (Ok(run), Ok(token)) = (
        std::env::var("LOOP_MANAGER_RUN"),
        std::env::var("LOOP_MANAGER_ATTEMPT_TOKEN"),
    ) else {
        return Ok(fail(
            json,
            "usage",
            "LOOP_MANAGER_RUN and LOOP_MANAGER_ATTEMPT_TOKEN are not set: `loop-manager finish` runs inside an agent session",
        ));
    };
    let data = match data_arg(data) {
        Ok(data) => data,
        Err(e) => return Ok(fail(json, "usage", &e)),
    };
    let body = json!({ "token": token, "outcome": outcome(result, summary, data) });
    let path = run_path(&run, "/finish");
    // A retry whose first try was committed but not answered gets the first reply. The key is a hash, so the
    // state never sees the token.
    let mut hasher = DefaultHasher::new();
    body.to_string().hash(&mut hasher);
    let key = format!("finish:{run}:{:016x}", hasher.finish());
    let retry = std::env::var("LOOP_MANAGER_FINISH_RETRY")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(FINISH_RETRY, Duration::from_secs);
    let give_up = tokio::time::Instant::now() + retry;
    let mut pause = Duration::from_millis(250);
    loop {
        match client.request("POST", &path, Some(&body), Some(&key)).await {
            Ok(reply) => {
                let answer = reply["answer"].as_str().unwrap_or_default();
                print(json, if json { &reply } else { &reply["answer"] });
                return Ok(if answer == "accepted" {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                });
            }
            Err(ClientError::Io(e)) if tokio::time::Instant::now() + pause > give_up => {
                let _ = fail(
                    json,
                    "no_daemon",
                    &format!("the daemon stayed unreachable for {} s: {e}", retry.as_secs()),
                );
                return Ok(ExitCode::from(EX_TEMPFAIL));
            }
            Err(ClientError::Io(_)) => {
                tokio::time::sleep(pause).await;
                pause = (pause * 2).min(Duration::from_secs(5));
            }
            Err(e) => return Err(e),
        }
    }
}

/// The `--data` of an outcome: JSON, or `@<file>` holding it; none is `null`.
pub(super) fn data_arg(data: Option<String>) -> Result<Value, String> {
    let Some(data) = data else { return Ok(Value::Null) };
    let text = match data.strip_prefix('@') {
        Some(file) => std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?,
        None => data,
    };
    serde_json::from_str(&text).map_err(|e| format!("--data: {e}"))
}

/// An outcome as the API takes it (contracts 1.3 `Outcome`).
pub(super) fn outcome(result: Verdict, summary: String, data: Value) -> Value {
    let result = match result {
        Verdict::Pass => "pass",
        Verdict::Fail => "fail",
    };
    json!({ "result": result, "summary": summary, "data": data })
}

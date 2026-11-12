//! Criterion 3: a formula of one exec step passes; exit 75 (`EX_TEMPFAIL`) fails the attempt and the next attempt
//! passes, while any other non-zero exit fails the formula.
//!
//! Retrying a failed attempt is a plugin decision; until the bundled `default-policy` exists, a plugin of one hook
//! that always asks for a new session stands in for it.

use super::exec_formula;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

pub(crate) async fn run(env: Arc<Env>) -> String {
    let d = Daemon::start(&env, |root| {
        write(
            root.join("config/aae/config.toml"),
            "projects = [\"shop\"]\n[[plugin]]\npath = \"retry\"\n",
        );
        write(
            root.join("config/aae/retry/plugin.toml"),
            "name = \"retry\"\nversion = \"0.1.0\"\n[[hook]]\non = \"run.attempt_failed\"\n\
             command = [\"sh\", \"-c\", \"echo '{\\\"decision\\\":\\\"new_session\\\"}'\"]\ntimeout = \"10s\"\n",
        );
        let once = root.join("once");
        let flaky = format!(
            "\"sh\", \"-c\", \"test -f '{}' && exit 0; touch '{}'; exit 75\"",
            once.display(),
            once.display()
        );
        exec_formula(root, "ok", "", "\"true\"");
        exec_formula(root, "flaky", "", &flaky);
        exec_formula(root, "broken", "", "\"sh\", \"-c\", \"exit 3\"");
    })
    .await;
    let (ok, flaky, broken) = (d.create("ok").await, d.create("flaky").await, d.create("broken").await);

    let view = d.until_state(&ok, "passed", WITHIN).await;
    assert_eq!(view["attempts"].as_array().map(Vec::len), Some(1), "{view}");

    let view = d.until_state(&flaky, "passed", WITHIN).await;
    let attempts = view["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 2, "exit 75 takes one more attempt: {view}");
    assert!(
        attempts[0]["failure_kind"].is_string(),
        "the first attempt failed: {view}"
    );
    assert!(attempts[1]["failure_kind"].is_null(), "{view}");

    let view = d.until_state(&broken, "failed", WITHIN).await;
    let attempts = view["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 1, "exit 3 fails the formula, not the attempt: {view}");
    String::new()
}

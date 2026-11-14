//! Criterion 3: a formula of one exec step passes; exit 75 (`EX_TEMPFAIL`) fails the attempt and the next attempt
//! passes, while any other non-zero exit fails the formula.
//!
//! The retry is the decision of the bundled `default-policy`, on by default: exit 75 is an `unknown` failure it
//! retries, here after 1 s instead of 30 s to keep the scenario short.

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
            "projects = [\"shop\"]\n[builtin.default-policy]\nunknown_after_s = 1\n",
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
    assert_eq!(
        view["outcome"]["result"], "pass",
        "the run view shows the outcome: {view}"
    );
    assert_eq!(view["outcome"]["data"]["exit_code"], 0, "{view}");

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
    assert_eq!(view["outcome"]["data"]["exit_code"], 3, "{view}");
    String::new()
}

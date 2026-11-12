//! Criterion 4, on the real `claude` against a fake Anthropic API: a turn that ends without the finisher does not
//! pass; the finisher of a stale attempt is rejected; a remark of a `run.finishing` hook sends the agent back, and
//! the run passes on its second go in the same attempt. Each part has its own daemon, so their plugins do not meet.

use super::claude_agent;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_testkit::anthropic::{Provider, Reply};
use serde_json::Value;
use std::{path::Path, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

/// Project `shop` with harness claude and plugin `hook`: one hook on `on` running `script` with `sh -c`.
fn hooked(root: &Path, on: &str, script: &str) {
    write(
        root.join("config/aae/config.toml"),
        "projects = [\"shop\"]\n[harness.claude]\n[[plugin]]\npath = \"hook\"\n",
    );
    let script = serde_json::to_string(script).unwrap();
    write(
        root.join("config/aae/hook/plugin.toml"),
        format!(
            "name = \"hook\"\nversion = \"0.1.0\"\n[[hook]]\non = \"{on}\"\ncommand = [\"sh\", \"-c\", {script}]\n\
             timeout = \"10s\"\n"
        ),
    );
}

/// The types of the events of `run`, read from the start of the journal through its terminal event.
async fn events_of(d: &Daemon, run: &str) -> Vec<(String, Value)> {
    let mut events = d.client.events(Some(1)).await.unwrap();
    let mut seen = Vec::new();
    loop {
        let next = tokio::time::timeout(WITHIN, events.next()).await;
        let event = next.expect("an event in time").unwrap().expect("the stream goes on");
        let envelope: Value = serde_json::from_str(&event.data).unwrap();
        if envelope["run"] == run {
            seen.push((event.event.clone(), envelope["data"].clone()));
            if ["run.passed", "run.failed", "run.cancelled"].contains(&event.event.as_str()) {
                return seen;
            }
        }
    }
}

async fn without_finisher(env: &Arc<Env>) {
    let provider = Provider::start(vec![Reply::Text("Done.".into())]).await;
    let d = Daemon::start(env, |root| claude_agent(root, "dev", &provider.base_url())).await;
    let run = d.create("dev").await;
    let events = events_of(&d, &run).await;
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert!(kinds.contains(&"run.turn_ended_without_outcome"), "{kinds:?}");
    assert_eq!(
        kinds.last(),
        Some(&"run.failed"),
        "a turn without the finisher does not pass"
    );
}

async fn stale_finisher(env: &Arc<Env>) {
    let scratch = tempfile::tempdir().unwrap();
    let (old, answer) = (scratch.path().join("token"), scratch.path().join("answer"));
    let aae = env.aae.display();
    let provider = Provider::start(vec![
        Reply::Bash(format!("printf %s \"$AAE_ATTEMPT_TOKEN\" > '{}'", old.display())),
        // Not an overflow: claude answers that with compaction requests of its own.
        Reply::Overloaded,
        // The second attempt first finishes with the token of the first one, then with its own.
        Reply::Bash(format!(
            "AAE_ATTEMPT_TOKEN=$(cat '{}') {aae} finish pass --summary old > '{}'; echo $? >> '{}'",
            old.display(),
            answer.display(),
            answer.display()
        )),
        Reply::Bash(format!("{aae} finish pass --summary new")),
        Reply::Text("Done.".into()),
    ])
    .await;
    let d = Daemon::start(env, |root| {
        claude_agent(root, "dev", &provider.base_url());
        hooked(root, "run.attempt_failed", "echo '{\"decision\":\"new_session\"}'");
    })
    .await;
    let run = d.create("dev").await;
    let view = d.until_state(&run, "passed", WITHIN).await;
    assert_eq!(view["attempts"].as_array().map(Vec::len), Some(2), "{view}");
    let answer = std::fs::read_to_string(&answer).unwrap();
    let lines: Vec<&str> = answer.lines().collect();
    assert_eq!(lines[0], "stale_attempt", "{answer}");
    assert_ne!(lines[1], "0", "a rejected finisher exits non-zero: {answer}");
    let events = events_of(&d, &run).await;
    assert_eq!(events.last().unwrap().1["outcome"]["summary"], "new");
}

async fn remark(env: &Arc<Env>) {
    let finish = |summary: &str| Reply::Bash(format!("{} finish pass --summary {summary}", env.aae.display()));
    let provider = Provider::start(vec![
        finish("first"),
        Reply::Text("Done.".into()),
        finish("second"),
        Reply::Text("Done.".into()),
    ])
    .await;
    let d = Daemon::start(env, |root| {
        claude_agent(root, "dev", &provider.base_url());
        let script = "if grep -q '\"remarks\":0'; then echo '{\"decision\":\"remark\",\"message\":\"Add a test.\"}'; \
                      else echo '{\"decision\":\"accept\"}'; fi";
        hooked(root, "run.finishing", script);
    })
    .await;
    let run = d.create("dev").await;
    let view = d.until_state(&run, "passed", WITHIN).await;
    assert_eq!(view["attempts"].as_array().map(Vec::len), Some(1), "{view}");
    let events = events_of(&d, &run).await;
    let finishing = events.iter().filter(|(k, _)| k == "run.finishing").count();
    assert_eq!(finishing, 2, "{events:?}");
    assert_eq!(events.last().unwrap().1["outcome"]["summary"], "second");
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    tokio::join!(without_finisher(&env), stale_finisher(&env), remark(&env));
    String::new()
}

//! Criterion 15: the example external plugin `examples/aae-plugin-review` gives its skill and harness hook only to the
//! one of two agents that attaches it, vetoes a `pass` whose summary lacks its word, and a failure of its hook does
//! not bring the daemon down.

use super::claude_agent;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_testkit::anthropic::{Provider, Reply};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);
const PLUGIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/aae-plugin-review");
/// What the plugin's harness hook and skill put before the model.
const REMINDER: &str = "Review reminder: check your work before you finish.";
const SKILL: &str = "The checks to make before declaring that a piece of work passes.";

/// The orchestrator config declaring the plugin, with its `require` parameter when given.
fn declare(root: &Path, require: Option<&str>) {
    let config = require.map(|r| format!("config = {{ require = \"{r}\" }}\n"));
    let text = format!(
        "projects = [\"shop\"]\n[harness.claude]\n[[plugin]]\npath = \"{PLUGIN}\"\n{}",
        config.unwrap_or_default()
    );
    write(root.join("config/aae/config.toml"), text);
}

/// Agent `dev` on `provider`, attaching the plugin.
fn dev(root: &Path, provider: &Provider) {
    claude_agent(root, "dev", &provider.base_url());
    let agent = root.join("config/aae/shop/agents/dev/agent.toml");
    let text = std::fs::read_to_string(&agent).unwrap();
    write(&agent, format!("plugins = [{{ name = \"review\" }}]\n{text}"));
}

/// The `run.failed` of `run`, waiting for it.
async fn failed(d: &Daemon, run: &str) -> Value {
    let seen = d
        .journal_until(WITHIN, |seen| {
            seen.iter().any(|(k, e)| k == "run.failed" && e["run"] == run)
        })
        .await;
    let failed = seen.iter().find(|(k, e)| k == "run.failed" && e["run"] == run);
    failed.unwrap().1["data"].clone()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = |summary: &str| {
        let finish = format!("{} finish pass --summary '{summary}'", env.aae.display());
        vec![Reply::Bash(finish), Reply::Text("Done.".into())]
    };
    let (vetoed, bare) = (
        Provider::start(finish("built")).await,
        Provider::start(finish("built and tested")).await,
    );
    let d = Daemon::start(&env, |root| {
        claude_agent(root, "bare", &bare.base_url());
        dev(root, &vetoed);
        declare(root, Some("tested"));
    })
    .await;

    let run = d.create("dev").await;
    let veto = failed(&d, &run).await;
    assert_eq!(veto["class"], "check", "{veto}");
    let reason = veto["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("must say the work is tested"), "{veto}");
    let other = d.create("bare").await;
    d.until_state(&other, "passed", WITHIN).await;

    let seen = |p: &Provider, text: &str| p.requests().iter().any(|r| r.body.to_string().contains(text));
    assert!(seen(&vetoed, REMINDER), "the attaching agent's hook ran");
    assert!(seen(&vetoed, SKILL), "the attaching agent has the skill");
    assert!(!seen(&bare, REMINDER), "the other agent has no hook of the plugin");
    assert!(!seen(&bare, SKILL), "the other agent has no skill of the plugin");

    // Without its parameter the veto hook fails: the run fails on it, the daemon goes on.
    let broken = Provider::start(finish("built and tested")).await;
    let root = d.dir.path().to_owned();
    dev(&root, &broken);
    declare(&root, None);
    d.post("/v1/config/reload", json!({})).await.unwrap();
    let run = d.create("dev").await;
    let failure = failed(&d, &run).await;
    assert_eq!(failure["reason"], "hook_failed:review", "{failure}");
    let seen = d
        .journal_until(WITHIN, |seen| seen.iter().any(|(k, _)| k == "plugin.hook_failed"))
        .await;
    let hook = &seen.iter().find(|(k, _)| k == "plugin.hook_failed").unwrap().1["data"];
    assert_eq!(
        (&hook["plugin"], &hook["error"]),
        (&json!("review"), &json!("exit")),
        "{hook}"
    );
    let pid = d.pid();
    assert_eq!(d.get("/v1/daemon").await["pid"], pid, "the daemon is up");
    format!(
        "veto: {reason}; skill and hook only in the attaching agent's session; a failed hook: {}",
        hook["stderr_tail"].as_str().unwrap_or_default().trim()
    )
}

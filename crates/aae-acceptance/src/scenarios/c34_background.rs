//! Criterion 34, on the real `claude` and `codex` against fake providers: two runs at once, each leaving a command
//! in the background. Run A on rig `web` declares its outcome: its step ends at once and its command is stopped
//! before it finishes. Run B on rig `api` declares none: its background is waited for, the stop of A's leaves it
//! alone (with codex both threads share one app-server), and B fails for the missing outcome. A command each run
//! detached from its harness is gone soon after its run ended.

use super::{c04_finisher::events_of, claude_agent, codex_agent};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_core::orphans::start_time;
use aae_testkit::{anthropic, openai};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);
/// The left command runs 20 s, then marks that it finished.
const LEFT: &str = "sh -c 'echo $$ > bg.pid; exec sleep 20'; touch bg-done";
/// A command that detaches itself: no background of the harness, it would outlive the run.
const DETACHED: &str = "setsid sh -c 'echo $$ > det.pid; exec sleep 300' >/dev/null 2>&1 &";

/// Declares `pass` only where the rig has the file `declare`.
fn declare(env: &Env) -> String {
    format!(
        "[ -e declare ] && {} finish pass --summary served; true",
        env.aae.display()
    )
}

/// The pid the left command wrote to `file`, once it is there (up to 10 s).
async fn pid_in(file: &Path) -> i32 {
    for _ in 0..100 {
        if let Some(pid) = std::fs::read_to_string(file).ok().and_then(|t| t.trim().parse().ok()) {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("no pid in {}", file.display());
}

/// Whether the process whose pid is in `file` is gone within `within`.
async fn gone(file: &Path, within: Duration) -> bool {
    let (pid, deadline) = (pid_in(file).await, tokio::time::Instant::now() + within);
    while start_time(pid).is_some() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    start_time(pid).is_none()
}

async fn two_runs(env: &Arc<Env>, harness: &str, agent: impl FnOnce(&Path)) {
    let d = Daemon::start(env, |root| {
        agent(root);
        let shop = root.join("config/aae/shop");
        write(
            shop.join("aae.toml"),
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        );
        write(shop.join("web/declare"), "");
        write(shop.join("api/.keep"), "");
    })
    .await;
    let (web, api) = (d.path("config/aae/shop/web"), d.path("config/aae/shop/api"));
    let a = d.create("dev").await;
    let body = json!({ "project": "shop", "rig": "api", "formula": "dev" });
    let b = d.post("/v1/runs", body).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();

    d.until_state(&a, "passed", WITHIN).await;
    assert!(
        !web.join("bg-done").exists(),
        "{harness}: A passed before its command ended"
    );
    let ten = Duration::from_secs(10);
    assert!(
        gone(&web.join("bg.pid"), ten).await,
        "{harness}: A's command is stopped"
    );
    let fifteen = Duration::from_secs(15);
    assert!(
        gone(&web.join("det.pid"), fifteen).await,
        "{harness}: A's detached command goes"
    );
    let events = events_of(&d, &a).await;
    assert!(
        events.iter().any(|(k, _)| k == "run.finishing"),
        "{harness}: {events:?}"
    );

    let events = events_of(&d, &b).await;
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        kinds.contains(&"run.turn_ended_without_outcome"),
        "{harness}: {kinds:?}"
    );
    assert_eq!(kinds.last(), Some(&"run.failed"), "{harness}: {kinds:?}");
    assert!(api.join("bg-done").exists(), "{harness}: B's background was waited for");
    assert!(!web.join("bg-done").exists(), "{harness}: A's command never finished");
    assert!(
        gone(&api.join("det.pid"), fifteen).await,
        "{harness}: B's detached command goes"
    );
}

async fn claude(env: &Arc<Env>) {
    use anthropic::Reply;
    let turn = vec![
        Reply::Bash(declare(env)),
        Reply::Background(LEFT.into()),
        Reply::Bash(DETACHED.into()),
        Reply::Text("Started.".into()),
    ];
    let provider = anthropic::Provider::start(vec![Reply::Turn(turn)]).await;
    two_runs(env, "claude", |root| claude_agent(root, "dev", &provider.base_url())).await;
}

async fn codex(env: &Arc<Env>) {
    use openai::Reply;
    // The second command yields its turn after about 10 s and runs on.
    let turn = vec![
        Reply::Command(declare(env)),
        Reply::Command(LEFT.into()),
        Reply::Command(DETACHED.into()),
        Reply::Text("Started.".into()),
    ];
    let provider = openai::Provider::start(vec![Reply::Turn(turn)]).await;
    two_runs(env, "codex", |root| codex_agent(root, "dev", &provider.base_url())).await;
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    tokio::join!(claude(&env), codex(&env));
    String::new()
}

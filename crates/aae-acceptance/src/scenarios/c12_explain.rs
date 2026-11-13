//! Criterion 12: "explain config" shows the layer of every value and the host layer in one line, through the API and
//! the CLI alike; on a stand-in harness home, one agent with the host layer and then, after a config reload, without
//! it differs in the session: only while it inherits does it get the host's `CLAUDE.md`.

use super::claude_agent;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_testkit::anthropic::{Provider, Reply};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);
const HOST_MARK: &str = "host-memory-mark-12";

/// Agent `dev` on `provider`, with the host layer or without it.
fn dev(root: &Path, provider: &Provider, inherit: bool) {
    claude_agent(root, "dev", &provider.base_url());
    let agent = root.join("config/aae/shop/agents/dev/agent.toml");
    let text = std::fs::read_to_string(&agent).unwrap();
    write(
        &agent,
        text.replace("inherit_host = false", &format!("inherit_host = {inherit}")),
    );
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = || {
        let finish = format!("{} finish pass --summary done", env.aae.display());
        vec![Reply::Bash(finish), Reply::Text("Done.".into())]
    };
    let (host, bare) = (Provider::start(finish()).await, Provider::start(finish()).await);
    // `HOME` of the daemon is the root of its tree: the stand-in harness home.
    // The rig lies under it, as rigs under a home do: claude reads `CLAUDE.md` up the rig's ancestors too.
    let d = Daemon::start(&env, |root| {
        dev(root, &host, true);
        write(root.join(".claude/CLAUDE.md"), format!("Always mention {HOST_MARK}.\n"));
    })
    .await;

    // The same agent with the host layer, then switched off by a config reload.
    let mut shown = Vec::new();
    for (provider, inherit, host_line) in [
        (
            &host,
            true,
            "host: inherited by the harness, its content is not visible to aae\n",
        ),
        (&bare, false, "host: off\n"),
    ] {
        if !inherit {
            dev(d.dir.path(), provider, false);
            d.post("/v1/config/reload", json!({})).await.unwrap();
        }
        let api = d.get("/v1/config/explain?project=shop&agent=dev").await;
        let text = api["text"].as_str().unwrap().to_owned();
        let cli = d.cli(&["config", "explain", "--project", "shop", "--agent", "dev"]);
        assert!(cli.status.success(), "{}", String::from_utf8_lossy(&cli.stderr));
        assert_eq!(
            String::from_utf8_lossy(&cli.stdout),
            text,
            "the CLI prints what the API explains"
        );
        for line in text.lines().filter(|l| !l.starts_with("host:")) {
            assert!(line.contains("  # "), "every value names its layer: {line}");
        }
        assert!(text.contains("model = \"claude-haiku-5-5\"  # agent\n"), "{text}");
        assert_eq!(text.lines().filter(|l| l.starts_with("host:")).count(), 1, "{text}");
        assert!(text.ends_with(host_line), "{text}");
        shown.push(text.lines().count());
        let run = d.create("dev").await;
        d.until_state(&run, "passed", WITHIN).await;
    }
    let seen = |p: &Provider| p.requests().iter().any(|r| r.body.to_string().contains(HOST_MARK));
    assert!(seen(&host), "the inheriting session got the host's CLAUDE.md");
    assert!(!seen(&bare), "the isolated session did not");
    format!(
        "explain: {} and {} lines, one host line each; the host CLAUDE.md reached the agent only while it inherited",
        shown[0], shown[1]
    )
}

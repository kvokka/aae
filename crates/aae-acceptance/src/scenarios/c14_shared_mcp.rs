//! Criterion 14: one HTTP MCP service that aae starts serves concurrent claude and codex sessions, each with its own
//! identity headers and the service's token; it runs as one process however many sessions use it. So does a stdio
//! MCP server behind the aae bridge, each call carrying its run in `_meta`.

use super::{claude_agent, codex_agent};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_testkit::{
    anthropic,
    mcp::{STDIO_TOOL, TOOL},
    openai,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(90);
const CLAUDE: &[&str] = &["c1", "c2"];
const CODEX: &[&str] = &["x1", "x2"];

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = format!("{} finish pass --summary built", env.aae.display());
    // Each session calls the tool, then the finisher, then ends: one plan serves every session at once.
    let claude = anthropic::Provider::start(vec![anthropic::Reply::Turn(vec![
        anthropic::Reply::Mcp(TOOL.into()),
        anthropic::Reply::Mcp(STDIO_TOOL.into()),
        anthropic::Reply::Bash(finish.clone()),
        anthropic::Reply::Text("Done.".into()),
    ])])
    .await;
    let codex = openai::Provider::start(vec![openai::Reply::Turn(vec![
        openai::Reply::Mcp(TOOL.into()),
        openai::Reply::Mcp(STDIO_TOOL.into()),
        openai::Reply::Command(finish),
        openai::Reply::Text("Done.".into()),
    ])])
    .await;
    let probe = std::env::current_exe().unwrap();
    let d = Daemon::start(&env, |root| {
        for name in CLAUDE {
            claude_agent(root, name, &claude.base_url());
            // With tool search on, claude defers MCP tools and the request lists none of them.
            let agent = root.join(format!("config/aae/shop/agents/{name}/agent.toml"));
            let text = fs::read_to_string(&agent).unwrap();
            write(&agent, format!("{text}ENABLE_TOOL_SEARCH = \"false\"\n"));
        }
        for name in CODEX {
            codex_agent(root, name, &codex.base_url());
        }
        let command = json!([probe, "--mcp-probe", root.join("probe.log")]);
        let stdio = json!([probe, "--mcp-stdio-probe", root.join("stdio.log")]);
        write(
            root.join("config/aae/config.toml"),
            format!(
                "projects = [\"shop\"]\n[harness.claude]\n[harness.codex]\n[[service]]\nname = \"probe\"\n\
                 command = {command}\nmcp = true\n[settings.mcp.notes]\ncommand = {stdio}\n"
            ),
        );
    })
    .await;

    let mut runs = Vec::new();
    for name in CLAUDE.iter().chain(CODEX) {
        runs.push((*name, d.create(name).await));
    }
    for (_, run) in &runs {
        d.until_state(run, "passed", WITHIN).await;
    }

    let text = fs::read_to_string(d.path("probe.log")).unwrap();
    let log: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let pids: BTreeSet<String> = log.iter().map(|l| l["pid"].to_string()).collect();
    assert_eq!(pids.len(), 1, "one service process: {pids:?}");
    assert!(
        log.iter().all(|l| l["authorized"] == true),
        "the service token in every request: {log:?}"
    );
    for (agent, run) in &runs {
        let call = log
            .iter()
            .find(|l| l["method"] == "tools/call" && l["run"] == run.as_str());
        let call = call.unwrap_or_else(|| panic!("no tool call of run {run} ({agent}): {log:?}"));
        let who = (&call["project"], &call["rig"], &call["agent"], &call["tool"]);
        assert_eq!(
            who,
            (
                &json!("shop"),
                &json!("web"),
                &json!(format!("shop.{agent}")),
                &json!(TOOL)
            ),
            "{call}"
        );
    }
    let stdio = stdio_server(&d, &runs);
    let seen = d
        .journal_until(WITHIN, |seen| {
            seen.iter().filter(|(k, _)| k == "run.passed").count() == runs.len()
        })
        .await;
    let of = |kind: &str, name: &str| -> Vec<&Value> {
        let service = seen
            .iter()
            .filter(|(k, e)| k == kind && e["data"]["service"]["name"] == name);
        service.map(|(_, e)| &e["data"]["service"]).collect()
    };
    for name in ["probe", "notes"] {
        let (starting, ready) = (of("service.starting", name), of("service.ready", name));
        assert_eq!(
            (starting.len(), ready.len()),
            (1, 1),
            "one start of the service {name}: {seen:?}"
        );
        assert!(of("service.failed", name).is_empty(), "{seen:?}");
    }
    format!(
        "{} claude and {} codex runs passed on one service process (pid {}); {} requests, all with the token and \
         each run's identity headers; {stdio}",
        CLAUDE.len(),
        CODEX.len(),
        pids.first().cloned().unwrap_or_default(),
        log.len()
    )
}

/// Checks the log of the stdio server behind the bridge: one process, initialized once, and a call of every run
/// with the run in `_meta`. Returns the report's words on it.
fn stdio_server(d: &Daemon, runs: &[(&str, String)]) -> String {
    let text = fs::read_to_string(d.path("stdio.log")).unwrap();
    let log: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let pids: BTreeSet<String> = log.iter().map(|l| l["pid"].to_string()).collect();
    assert_eq!(pids.len(), 1, "one stdio server process: {pids:?}");
    let initialize = log.iter().filter(|l| l["method"] == "initialize").count();
    assert_eq!(initialize, 1, "the bridge initializes the server once: {log:?}");
    for (agent, run) in runs {
        let calls = log
            .iter()
            .filter(|l| l["method"] == "tools/call" && l["meta"]["aae/run"] == run.as_str());
        assert_eq!(calls.count(), 1, "one stdio call of run {run} ({agent}): {log:?}");
    }
    format!(
        "one stdio MCP server behind the aae bridge (pid {}) served every run once each, initialized once, the run \
         in each call's _meta",
        pids.first().cloned().unwrap_or_default()
    )
}

//! Criterion 14: one HTTP MCP service that aae starts serves concurrent claude and codex sessions, each with its own
//! identity headers and the service's token; it runs as one process however many sessions use it.

use super::{claude_agent, codex_agent};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_testkit::{anthropic, mcp::TOOL, openai};
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
        anthropic::Reply::Bash(finish.clone()),
        anthropic::Reply::Text("Done.".into()),
    ])])
    .await;
    let codex = openai::Provider::start(vec![openai::Reply::Turn(vec![
        openai::Reply::Mcp(TOOL.into()),
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
        write(
            root.join("config/aae/config.toml"),
            format!(
                "projects = [\"shop\"]\n[harness.claude]\n[harness.codex]\n[settings.mcp.probe]\ncommand = {command}\n\
                 url = \"http://127.0.0.1:${{AAE_SERVICE_PORT}}/mcp\"\nscope = \"orchestrator\"\n"
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
    let seen = d
        .journal_until(WITHIN, |seen| {
            seen.iter().filter(|(k, _)| k == "run.passed").count() == runs.len()
        })
        .await;
    let of = |kind: &str| -> Vec<&Value> {
        let probe = seen
            .iter()
            .filter(|(k, e)| k == kind && e["data"]["service"]["name"] == "probe");
        probe.map(|(_, e)| &e["data"]["service"]).collect()
    };
    let (starting, ready) = (of("service.starting"), of("service.ready"));
    assert_eq!(
        (starting.len(), ready.len()),
        (1, 1),
        "one start of the service: {seen:?}"
    );
    assert!(of("service.failed").is_empty(), "{seen:?}");
    format!(
        "{} claude and {} codex runs passed on one service process (pid {}); {} requests, all with the token and \
         each run's identity headers",
        CLAUDE.len(),
        CODEX.len(),
        pids.first().cloned().unwrap_or_default(),
        log.len()
    )
}

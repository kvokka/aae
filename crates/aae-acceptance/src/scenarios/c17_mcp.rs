//! Criterion 17, the MCP frontend from a real model: a session of the cheapest claude model calls `list_formulas` on
//! the daemon's own MCP endpoint and reports what it listed through the finisher. The formula it must name appears
//! nowhere but in that tool's answer. Costs money; runs only on an explicit spec run.

use super::exec_formula;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{sync::Arc, time::Duration};

/// Named only by the tool's answer.
const HIDDEN: &str = "zebra-audit";

pub(crate) async fn run(env: Arc<Env>) -> String {
    let aae = env.aae.display().to_string();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let d = Daemon::start_on_host(&env, |root| {
        write(
            root.join("config/aae/config.toml"),
            format!("projects = [\"shop\"]\napi_tcp = \"127.0.0.1:{port}\"\n[harness.claude]\n"),
        );
        // With tool search on, claude would defer the MCP tools and list none of them to the model.
        write(
            root.join("config/aae/shop/agents/dev/agent.toml"),
            "harness = \"claude\"\nmodel = \"haiku\"\npermission_mode = \"bypassPermissions\"\n\
             [env]\nENABLE_TOOL_SEARCH = \"false\"\n[mcp.aae]\nurl = \"${AAE_API}/mcp\"\n",
        );
        write(
            root.join("config/aae/shop/formulas/dev.toml"),
            "name = \"dev\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"dev.md\"\n",
        );
        write(
            root.join("config/aae/shop/formulas/dev.md"),
            format!(
                "Call the tool `list_formulas` of the MCP server `aae` once. Then run this shell command once, with \
                 the names of all the formulas it listed, comma-separated, in place of NAMES: \
                 `{aae} finish pass --summary \"NAMES\"`. Then stop."
            ),
        );
        exec_formula(root, HIDDEN, "", "\"true\"");
    })
    .await;
    let run = d.create("dev").await;
    d.until_state(&run, "passed", Duration::from_secs(300)).await;
    let journal = d
        .journal_until(Duration::from_secs(10), |seen| {
            seen.iter().any(|(k, e)| k == "run.passed" && e["run"] == run.as_str())
        })
        .await;
    let passed = journal
        .iter()
        .find(|(k, e)| k == "run.passed" && e["run"] == run.as_str())
        .map(|(_, e)| &e["data"]["outcome"]["summary"]);
    let summary = passed.and_then(|s| s.as_str()).unwrap_or_default();
    assert!(
        summary.contains(HIDDEN),
        "the summary names what the tool listed: {passed:?}"
    );
    format!("claude, model haiku: the session listed the formulas over MCP and reported \"{summary}\"")
}

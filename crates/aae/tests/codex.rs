//! A prompt run through a real `aae daemon` and the real `codex app-server` on the fake Responses API (spec §7.8,
//! §15 criteria 2 and 9): the thread calls the finisher with `exec_command`, the run passes. No model, no account.

mod common;

use aae_testkit::openai::{Provider, Reply};
use common::Daemon;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

/// A daemon whose `build` formula is a prompt run of a codex agent on the fake `provider`.
async fn daemon(provider: &Provider) -> Daemon {
    daemon_with(provider, "", false, "").await
}

/// The same with `orchestrator` before the orchestrator config, `inherit_host` and `agent` after the agent's config.
async fn daemon_with(provider: &Provider, orchestrator: &str, inherit_host: bool, agent: &str) -> Daemon {
    let base_url = provider.base_url();
    Daemon::start(|dir| {
        let shop = dir.join("config/aae/shop");
        let files = [
            (
                dir.join("config/aae/config.toml"),
                format!("{orchestrator}projects = [\"shop\"]\n[harness.codex]\n"),
            ),
            (
                shop.join("agents/dev/agent.toml"),
                // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
                format!(
                    "harness = \"codex\"\nmodel = \"gpt-fake\"\npermission_mode = \"never\"\ninherit_host = {inherit_host}\n\
                     [native.codex]\nmodel_provider = \"fake\"\nsandbox_mode = \"danger-full-access\"\n\
                     [native.codex.model_providers.fake]\nname = \"fake\"\nbase_url = \"{base_url}\"\n\
                     wire_api = \"responses\"\n{agent}"
                ),
            ),
            (
                shop.join("formulas/build.toml"),
                "name = \"build\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n".to_owned(),
            ),
            (shop.join("formulas/build.md"), "Build {{ rig.name }}.".to_owned()),
        ];
        for (path, text) in files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_codex_thread_calls_the_mcp_of_the_daemon_over_tcp() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let replies = vec![
        Reply::Mcp("list_formulas".into()),
        Reply::Command(finish),
        Reply::Text("Done.".into()),
    ];
    let provider = Provider::start(replies).await;
    let orchestrator = format!("api_tcp = \"127.0.0.1:{port}\"\n");
    let d = daemon_with(&provider, &orchestrator, false, "[mcp.aae]\nurl = \"${AAE_API}/mcp\"\n").await;
    let run = d.create("build", None).await;
    d.until_state(&run, "passed").await;
    // The second model request carries the output of the `list_formulas` call: the daemon's formulas.
    let output = provider.requests()[1].body["input"].to_string();
    assert!(
        output.contains("function_call_output") && output.contains("slow"),
        "{output}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_codex_thread_that_calls_the_finisher_passes_its_run() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let provider = Provider::start(vec![Reply::Command(finish), Reply::Text("Done.".into())]).await;
    let d = daemon(&provider).await;
    let run = d.create("build", None).await;
    let view = d.until_state(&run, "passed").await;
    assert_eq!(view["step_kind"], "prompt");
    // Only the finisher passes a prompt run: the second model request carries the output of its command.
    let requests = provider.requests();
    let output = requests[1].body["input"].to_string();
    assert!(output.contains("function_call_output"), "{output}");
}

/// A codex agent with a skill that inherits the host runs in a `CODEX_HOME` of its own linking the host's: the model
/// sees the host's `AGENTS.md`, the host's skill and the agent's, and the linked `config.toml` is still a link after
/// the turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_agent_with_a_skill_inheriting_the_host_sees_the_hosts_files() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let provider = Provider::start(vec![Reply::Command(finish), Reply::Text("Done.".into())]).await;
    let skill = tempfile::tempdir().unwrap();
    let text = "---\nname: mail\ndescription: Sends the zq7-mail marker.\n---\nSend it.\n";
    fs::write(skill.path().join("SKILL.md"), text).unwrap();
    let agent = format!("[skills.mail]\npath = \"{}\"\n", skill.path().display());
    let d = daemon_with(&provider, "", true, &agent).await;
    let host = d.dir.path().join(".codex");
    fs::create_dir_all(host.join("skills/notes")).unwrap();
    let text = "---\nname: notes\ndescription: Keeps the zq8-notes marker.\n---\nKeep it.\n";
    fs::write(host.join("skills/notes/SKILL.md"), text).unwrap();
    fs::write(host.join("AGENTS.md"), "Mind the zq9-agents marker.\n").unwrap();
    fs::write(host.join("config.toml"), "").unwrap();
    let run = d.create("build", None).await;
    d.until_state(&run, "passed").await;
    let first = provider.requests()[0].body.to_string();
    for marker in ["zq7-mail", "zq8-notes", "zq9-agents"] {
        assert!(first.contains(marker), "{marker} in {first}");
    }
    let homes = fs::read_dir(d.dir.path().join("state/aae/codex")).unwrap();
    let homes: Vec<_> = homes.map(|e| e.unwrap().path().join("codex-home")).collect();
    assert_eq!(homes.len(), 1, "{homes:?}");
    let config = fs::read_link(homes[0].join("config.toml")).unwrap();
    assert_eq!(config, host.join("config.toml"), "still a link to the host's");
}

/// The processes whose environment holds a path under `dir` and whose command line contains `needle`.
fn processes(dir: &Path, needle: &str) -> Vec<i32> {
    let dir = dir.to_string_lossy();
    let entries = fs::read_dir("/proc").unwrap().flatten();
    entries
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            let cmd = String::from_utf8_lossy(&fs::read(e.path().join("cmdline")).ok()?).replace('\0', " ");
            let env = String::from_utf8_lossy(&fs::read(e.path().join("environ")).ok()?).into_owned();
            (cmd.contains(needle) && env.contains(&*dir)).then_some(pid)
        })
        .collect()
}

/// Criterion 9 for codex: `kill -9` of the app-server while a command runs is typed within a second, the thread is
/// resumed in a new app-server and the run reaches its outcome.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_killed_in_a_command_is_resumed_and_its_run_passes() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let plan = vec![Reply::Command("sleep 20".into()), Reply::Task(finish)];
    let provider = Provider::start(plan).await;
    let d = daemon(&provider).await;
    let mut events = d.client.events(Some(1)).await.unwrap();
    let run = d.create("build", None).await;
    let started = Instant::now();
    let server = loop {
        let servers = processes(d.dir.path(), "app-server");
        if let [server] = servers[..]
            && !processes(d.dir.path(), "sleep 20").is_empty()
        {
            break server;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no command running: {servers:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    // Typed is when the core recorded the failure: the `ts` of its event, against the wall clock of the kill.
    let killed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let status = std::process::Command::new("kill")
        .args(["-9", &server.to_string()])
        .status();
    assert!(status.unwrap().success());
    let envelope = loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next()).await;
        let event = event.expect("an event in time").unwrap().expect("the stream goes on");
        if event.event == "run.attempt_failed" {
            break serde_json::from_str::<Value>(&event.data).unwrap();
        }
    };
    let failure = &envelope["data"]["failure"];
    assert_eq!(failure["kind"], "harness_crash", "{failure}");
    let at = aae_core::parse_rfc3339_ms(envelope["ts"].as_str().unwrap()).unwrap();
    let typed = at - i64::try_from(killed.as_millis()).unwrap();
    assert!(typed <= 1000, "typed in {typed} ms");
    d.until_state(&run, "passed").await;
}

/// Criterion 8 for codex: after `kill -9` of the daemon mid-turn, the next daemon resumes the same thread in a new
/// app-server, finds the prompt unanswered by its `clientId` and reconciles; the run passes.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_thread_is_resumed_after_a_daemon_restart_and_its_run_passes() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let plan = vec![Reply::Command("sleep 20".into()), Reply::Task(finish)];
    let provider = Provider::start(plan).await;
    let mut d = daemon(&provider).await;
    let run = d.create("build", None).await;
    let started = Instant::now();
    while processes(d.dir.path(), "sleep 20").is_empty() {
        assert!(started.elapsed() < Duration::from_secs(30), "no command running");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    d.child.kill().unwrap();
    d.child.wait().unwrap();
    d.child = common::spawn(d.dir.path());
    while d.client.request("GET", "/v1/daemon", None, None).await.is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(40),
            "the daemon did not come up"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let view = d.until_state(&run, "passed").await;
    let attempts = view["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{view}");
    assert!(attempts[0]["session"].is_object(), "{view}");
    assert_eq!(attempts[0]["session"], attempts[1]["session"], "the same thread");
    // The reconciling turn carries the history of the first one: the command it was running.
    let last = provider.requests().last().unwrap().body["input"].to_string();
    assert!(last.contains("sleep 20"), "{last}");
    assert!(last.contains("previous turn was interrupted"), "{last}");
}

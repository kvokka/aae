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
    let base_url = provider.base_url();
    Daemon::start(|dir| {
        let shop = dir.join("config/aae/shop");
        let files = [
            (
                dir.join("config/aae/config.toml"),
                "projects = [\"shop\"]\n[harness.codex]\n".to_owned(),
            ),
            (
                shop.join("agents/dev/agent.toml"),
                // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
                format!(
                    "harness = \"codex\"\nmodel = \"gpt-fake\"\npermission_mode = \"never\"\ninherit_host = false\n\
                     [native.codex]\nmodel_provider = \"fake\"\nsandbox_mode = \"danger-full-access\"\n\
                     [native.codex.model_providers.fake]\nname = \"fake\"\nbase_url = \"{base_url}\"\n\
                     wire_api = \"responses\"\n"
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

/// The `codex app-server` processes whose environment holds a path under `dir`.
fn app_servers(dir: &Path) -> Vec<i32> {
    let dir = dir.to_string_lossy();
    let entries = fs::read_dir("/proc").unwrap().flatten();
    entries
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            let cmd = String::from_utf8_lossy(&fs::read(e.path().join("cmdline")).ok()?).replace('\0', " ");
            let env = String::from_utf8_lossy(&fs::read(e.path().join("environ")).ok()?).into_owned();
            (cmd.contains("app-server") && env.contains(&*dir)).then_some(pid)
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
        let servers = app_servers(d.dir.path());
        if let [server] = servers[..]
            && provider.requests().len() == 1
        {
            break server;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no command running: {servers:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    // The command itself is running once the first model request has been answered and a moment has passed.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let killed = Instant::now();
    let status = std::process::Command::new("kill")
        .args(["-9", &server.to_string()])
        .status();
    assert!(status.unwrap().success());
    let failure = loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next()).await;
        let event = event.expect("an event in time").unwrap().expect("the stream goes on");
        if event.event == "run.attempt_failed" {
            let envelope: Value = serde_json::from_str(&event.data).unwrap();
            break envelope["data"]["failure"].clone();
        }
    };
    let typed = killed.elapsed();
    assert_eq!(failure["kind"], "harness_crash", "{failure}");
    assert!(typed <= Duration::from_secs(1), "typed in {typed:?}");
    d.until_state(&run, "passed").await;
}

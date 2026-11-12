//! A prompt run through a real `aae daemon` and the real `codex app-server` on the fake Responses API (spec §7.8,
//! §15 criterion 2): the thread calls the finisher with `exec_command`, the run passes. No model, no account.

mod common;

use aae_testkit::openai::{Provider, Reply};
use common::Daemon;
use std::fs;

#[tokio::test(flavor = "multi_thread")]
async fn a_codex_thread_that_calls_the_finisher_passes_its_run() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let provider = Provider::start(vec![Reply::Command(finish), Reply::Text("Done.".into())]).await;
    let base_url = provider.base_url();
    let d = Daemon::start(|dir| {
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
    .await;
    let run = d.create("build", None).await;
    let view = d.until_state(&run, "passed").await;
    assert_eq!(view["step_kind"], "prompt");
    // Only the finisher passes a prompt run: the second model request carries the output of its command.
    let requests = provider.requests();
    let output = requests[1].body["input"].to_string();
    assert!(output.contains("function_call_output"), "{output}");
}

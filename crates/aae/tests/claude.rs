//! A prompt run through a real `aae daemon` and the real `claude` on the fake Anthropic API (spec §7.8, §15
//! criterion 2): the session calls the finisher with its Bash tool, the run passes. No model, no account.

mod common;

use aae_testkit::anthropic::{Provider, Reply};
use common::Daemon;
use std::fs;

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_session_that_calls_the_finisher_passes_its_run() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let provider = Provider::start(vec![Reply::Bash(finish), Reply::Text("Done.".into())]).await;
    let base_url = provider.base_url();
    let d = Daemon::start(|dir| {
        let shop = dir.join("config/aae/shop");
        let files = [
            (
                dir.join("config/aae/config.toml"),
                "projects = [\"shop\"]\n[harness.claude]\n".to_owned(),
            ),
            (
                shop.join("agents/dev/agent.toml"),
                format!(
                    "harness = \"claude\"\nmodel = \"claude-haiku-5-5\"\npermission_mode = \"bypassPermissions\"\n\
                     inherit_host = false\n[env]\nANTHROPIC_BASE_URL = \"{base_url}\"\nANTHROPIC_API_KEY = \"fake\"\n\
                     CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n"
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
    // Only the finisher passes a prompt run: the second model request carries the result of its Bash call.
    let requests = provider.requests();
    let result = requests[1].body["messages"].to_string();
    assert!(result.contains("tool_result"), "{result}");
}

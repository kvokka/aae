//! Criterion 2, the claude half: a formula of one prompt step passes through a real run of the cheapest claude
//! model with the host login, ending `pass` by the finisher. Costs money; runs only on an explicit spec run.
//! The codex half needs a codex login on the host and comes with the codex contract tests.

use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{sync::Arc, time::Duration};

pub(crate) async fn run(env: Arc<Env>) -> String {
    let aae = env.aae.display().to_string();
    let d = Daemon::start_on_host(&env, |root| {
        write(
            root.join("config/aae/config.toml"),
            "projects = [\"shop\"]\n[harness.claude]\n",
        );
        write(
            root.join("config/aae/shop/agents/dev/agent.toml"),
            "harness = \"claude\"\nmodel = \"haiku\"\npermission_mode = \"bypassPermissions\"\n",
        );
        write(
            root.join("config/aae/shop/formulas/dev.toml"),
            "name = \"dev\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"dev.md\"\n",
        );
        write(
            root.join("config/aae/shop/formulas/dev.md"),
            format!("Run this shell command once and nothing else: `{aae} finish pass --summary done`. Then stop."),
        );
    })
    .await;
    let run = d.create("dev").await;
    let view = d.until_state(&run, "passed", Duration::from_secs(300)).await;
    assert_eq!(view["attempts"].as_array().map(Vec::len), Some(1), "{view}");
    "claude, model haiku".to_owned()
}

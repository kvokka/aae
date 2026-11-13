//! Criterion 4a on the cheapest real claude model: the finisher is called while a background subagent of the session
//! still works; the run passes only once the session is idle without background, so never before the subagent is
//! done. Costs money; runs only on an explicit spec run.

use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{fs, sync::Arc, time::Duration};

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
            format!(
                "Do exactly these two steps, in this order, and nothing else.\n\
                 1. Start a subagent with the Task tool in the background (run_in_background: true). Its task: \
                 run the shell command `sleep 30 && touch background-done` in the current directory, then reply \
                 \"done\".\n\
                 2. Right after starting it, without waiting for it, run this shell command once: \
                 `touch finish-called && {aae} finish pass --summary started`.\n\
                 Then stop and wait."
            ),
        );
    })
    .await;
    let rig = d.dir.path().join("config/aae/shop/web");
    let run = d.create("dev").await;
    d.until_state(&run, "passed", Duration::from_secs(300)).await;
    assert!(
        rig.join("background-done").exists(),
        "the run passed before the background subagent ended"
    );
    let modified = |name: &str| {
        fs::metadata(rig.join(name))
            .and_then(|m| m.modified())
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let (finish, background) = (modified("finish-called"), modified("background-done"));
    let ahead = background
        .duration_since(finish)
        .expect("the model ran the background task before calling the finisher; the check proves nothing");
    assert!(
        ahead >= Duration::from_secs(10),
        "the subagent was not in the background: it ended {ahead:?} after the finisher"
    );
    format!(
        "claude, model haiku: the finisher came {:.0} s before the background subagent ended; the run passed after it",
        ahead.as_secs_f64()
    )
}

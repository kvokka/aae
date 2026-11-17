//! Criterion 25: a failing check sends the run back to the step it names with its remarks, and the next round
//! passes; a check that keeps failing fails the run exactly after the formula's rounds, its remarks in the result.

use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{path::Path, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

/// Formula `name` of project `shop`: `plan` logs the remarks it got, `check` (back to `plan`) fails with a remark
/// on its first `fails` runs.
fn formula(root: &Path, name: &str, rounds: u32, fails: u32) {
    let (log, count) = (root.join(format!("{name}.log")), root.join(format!("{name}.count")));
    let (log, count) = (log.display(), count.display());
    let plan = format!("printf 'plan [%s]\\n' \"$LOOP_MANAGER_REMARKS\" >> '{log}'");
    let check = format!(
        "n=$(($(cat '{count}' 2>/dev/null || echo 0) + 1)); echo $n > '{count}'; \
         [ $n -gt {fails} ] || {{ echo 'totals are wrong'; exit 1; }}"
    );
    let step = |name: &str, script: &str, back: &str| {
        format!(
            "[[step]]\nname = \"{name}\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {}]\n{back}",
            serde_json::json!(script)
        )
    };
    let text = format!(
        "name = \"{name}\"\nrounds = {rounds}\n{}{}",
        step("plan", &plan, ""),
        step("check", &check, "back_to = \"plan\"\n")
    );
    write(
        root.join(format!("config/loop-manager/shop/formulas/{name}.toml")),
        text,
    );
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let d = Daemon::start(&env, |root| {
        write(root.join("config/loop-manager/config.toml"), "projects = [\"shop\"]\n");
        formula(root, "mends", 3, 1);
        formula(root, "stubborn", 2, 99);
    })
    .await;
    let (mends, stubborn) = (d.create("mends").await, d.create("stubborn").await);
    let remark = "check: exit 1\ntotals are wrong";
    let explain = d.cli(&[
        "config",
        "explain",
        "--project",
        "shop",
        "--formula",
        "stubborn",
        "--rig",
        "web",
    ]);
    let text = String::from_utf8_lossy(&explain.stdout);
    assert!(
        text.contains("# a check: on fail, back to `plan`, at most 2 rounds"),
        "{text}"
    );

    let view = d.until_state(&mends, "passed", WITHIN).await;
    let log = std::fs::read_to_string(d.path("mends.log")).expect("the log of plan");
    assert_eq!(log, format!("plan []\nplan [{remark}]\n"), "{view}");

    let view = d.until_state(&stubborn, "failed", WITHIN).await;
    let log = std::fs::read_to_string(d.path("stubborn.log")).expect("the log of plan");
    assert_eq!(log, format!("plan []\nplan [{remark}]\n"), "two rounds, no third");
    assert_eq!(
        view["reason"],
        format!("still failing after 2 rounds: {remark}"),
        "{view}"
    );
    "passed in round 2; failed after exactly 2 rounds with the remarks".into()
}

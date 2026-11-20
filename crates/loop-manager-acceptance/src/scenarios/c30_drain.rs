//! Criterion 30 on the real br, with no model: an exec step `plan` files five beads under the root bead of a run, a
//! drain step gives them runs of `work`. With no `pool` one bead is in work at a time, with `pool = 2` two at once,
//! each in a worktree; a failed bead stops nothing under `continue`; `soft_stop` lets the started bead end across a
//! `kill -9` and gives no new one; `hard_stop` cancels the started bead; a stopped step fails the run.

use super::tracker;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

const WITHIN: Duration = Duration::from_secs(60);

/// Formula `name`: `plan` files five beads under the run's bead, `drain` gives them runs of `work` with `keys`.
fn ship(name: &str, keys: &str) -> String {
    format!(
        "name = \"{name}\"\n[inputs]\ntask = {{ type = \"task\", required = true }}\n\
         [[step]]\nname = \"plan\"\nkind = \"exec\"\ncommand = [\"sh\", \"{{dir}}/plan.sh\"]\n\
         [[step]]\nname = \"drain\"\nkind = \"drain\"\nformula = \"work\"\n{keys}"
    )
}

/// Each run of `work` in a worktree of its own, so the runs of a pool never write in one directory.
const WORK: &str = "name = \"work\"\nworktree = true\n[inputs]\nbead = { type = \"task\", required = true }\n\
    [[step]]\nname = \"do\"\nkind = \"exec\"\ncommand = [\"sh\", \"{dir}/work.sh\"]\n";

const PLAN: &str = "set -e\nfor i in 1 2 3 4 5; do br create \"B$i\" --parent \"$LOOP_MANAGER_BEAD\" --silent; done\n";

/// The first run after `fail` appears fails at once (the rename lets one of two runs started together take it); any
/// other logs its bead in `started` and holds until `go`.
const WORK_SH: &str = "D={dir}\nif mv $D/fail $D/failed 2>/dev/null; then exit 3; fi\necho \"$LOOP_MANAGER_BEAD\" >> $D/started\n\
    while [ ! -e $D/go ]; do sleep 0.05; done\n";

/// The children of `run`: `(bead, state)`.
async fn kids(d: &Daemon, run: &str) -> Vec<(String, String)> {
    let kids = d.get(&format!("/v1/runs?parent={run}")).await;
    let kids = kids.as_array().expect("runs");
    let pair = |k: &Value| {
        (
            k["bead"].as_str().unwrap_or_default().into(),
            k["state"].as_str().unwrap_or_default().into(),
        )
    };
    kids.iter().map(pair).collect()
}

fn alive(kids: &[(String, String)]) -> usize {
    kids.iter()
        .filter(|(_, s)| !matches!(s.as_str(), "passed" | "failed" | "cancelled"))
        .count()
}

/// Waits until `ok` holds of the children of `run`.
async fn until_kids(
    d: &Daemon,
    run: &str,
    what: &str,
    ok: impl Fn(&[(String, String)]) -> bool,
) -> Vec<(String, String)> {
    let deadline = Instant::now() + WITHIN;
    loop {
        let kids = kids(d, run).await;
        if ok(&kids) {
            return kids;
        }
        assert!(Instant::now() < deadline, "{what}: {kids:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn lines(path: &Path) -> usize {
    fs::read_to_string(path).unwrap_or_default().lines().count()
}

/// Starts `formula` on a task of its own, after clearing `go` and `started` and, if `fail`, arming one failure.
async fn start(d: &Daemon, formula: &str, fail: bool) -> String {
    for name in ["go", "started"] {
        let _ = fs::remove_file(d.path(name));
    }
    if fail {
        write(d.path("fail"), "");
    }
    let body = json!({ "project": "shop", "formula": formula, "inputs": { "task": format!("Ship {formula}") } });
    let reply = d.post("/v1/runs", body).await.unwrap();
    reply["run"].as_str().expect("a run").to_owned()
}

/// The states of the children, sorted: the API lists the newest first.
fn states(kids: &[(String, String)]) -> Vec<&str> {
    let mut states: Vec<&str> = kids.iter().map(|(_, s)| s.as_str()).collect();
    states.sort_unstable();
    states
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let mut d = Daemon::start(&env, |root| {
        write(root.join("config/loop-manager/config.toml"), "projects = [\"shop\"]\n");
        let dir = root.display().to_string();
        let formulas = root.join("config/loop-manager/shop/formulas");
        for (name, keys) in [
            ("one", ""),
            ("two", "pool = 2\n"),
            ("soft", "pool = 2\non_item_failure = \"soft_stop\"\n"),
            ("hard", "pool = 2\non_item_failure = \"hard_stop\"\n"),
        ] {
            write(
                formulas.join(format!("{name}.toml")),
                ship(name, keys).replace("{dir}", &dir),
            );
        }
        write(formulas.join("work.toml"), WORK.replace("{dir}", &dir));
        write(root.join("plan.sh"), PLAN);
        write(root.join("work.sh"), WORK_SH.replace("{dir}", &dir));
        tracker(root);
        // A worktree needs a commit to branch from.
        let git = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ])
            .current_dir(root.join("config/loop-manager/shop"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status();
        assert!(git.is_ok_and(|s| s.success()), "git commit");
    })
    .await;
    let started = d.path("started");

    // No pool: the bead in work is the only one given.
    let one = start(&d, "one", false).await;
    until_kids(&d, &one, "a bead in work", |_| lines(&started) == 1).await;
    assert_eq!(kids(&d, &one).await.len(), 1, "no pool, one bead at a time");
    write(d.path("go"), "");
    d.until_state(&one, "passed", WITHIN).await;
    assert_eq!(states(&kids(&d, &one).await), ["passed"; 5]);

    // `pool = 2`: the first fails, the next two work at once, the rest wait; `continue` lets them all run.
    let two = start(&d, "two", true).await;
    let kids_two = until_kids(&d, &two, "two at work", |k| k.len() >= 3 && lines(&started) == 2).await;
    assert_eq!((kids_two.len(), alive(&kids_two)), (3, 2), "pool 2: {kids_two:?}");
    write(d.path("go"), "");
    d.until_state(&two, "passed", WITHIN).await;
    assert_eq!(
        states(&kids(&d, &two).await),
        ["failed", "passed", "passed", "passed", "passed"]
    );

    check_stop_policies(&mut d, &started).await;
    "five beads: no pool one at a time, pool 2 two at work at once, continue ran all; soft_stop ended the started across kill -9 and \
     failed with 3 not started; hard_stop cancelled the started"
        .into()
}

async fn check_stop_policies(d: &mut Daemon, started: &Path) {
    // `soft_stop`: the started bead ends across a kill -9, no new bead is given, the run fails.
    let soft = start(d, "soft", true).await;
    until_kids(d, &soft, "the second in work", |k| k.len() == 2 && lines(started) == 1).await;
    d.restart().await;
    write(d.path("go"), "");
    let view = d.until_state(&soft, "failed", WITHIN).await;
    assert_eq!(states(&kids(d, &soft).await), ["failed", "passed"], "{view}");
    let reason = view["reason"].as_str().unwrap_or_default();
    assert!(
        reason.starts_with("stopped on ") && reason.ends_with("3 not started"),
        "{view}"
    );

    // `hard_stop`: the started bead is cancelled, the run fails.
    let hard = start(d, "hard", true).await;
    let view = d.until_state(&hard, "failed", WITHIN).await;
    assert_eq!(states(&kids(d, &hard).await), ["cancelled", "failed"], "{view}");
}

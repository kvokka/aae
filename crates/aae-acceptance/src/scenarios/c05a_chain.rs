//! Criterion 5a: a chain without a model. An exec planner creates an epic of three beads with br; a project trigger
//! on `bead.ready` starts the executor on the next ready bead of that epic; every bead ends closed by its run, and
//! all four runs share the planner's correlation: br attributes the planner's writes to its attempt by `BD_ACTOR`.

use super::{br, exec_formula, tracker};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

const TRIGGER: &str = "[[trigger]]\nname = \"next\"\non = \"bead.ready\"\nwhere = { \"bead.labels\" = \"auto\" }\n\
                       run = \"work\"\nrig = \"web\"\ninputs = { bead = { next_ready = true, epic = \"{{ event.bead.epic }}\" } }\n";

pub(crate) async fn run(env: Arc<Env>) -> String {
    let d = Daemon::start(&env, |root| {
        write(
            root.join("config/aae/shop/aae.toml"),
            format!("[rigs.web]\npath = \"web\"\n{TRIGGER}"),
        );
        let plan = "set -e; epic=$(br create Epic -t epic --silent); \
                    for n in 1 2 3; do br create \"Task $n\" --parent \"$epic\" -l auto --silent; done";
        exec_formula(
            root,
            "plan",
            "",
            &format!("\"sh\", \"-c\", {}", serde_json::to_string(plan).unwrap()),
        );
        exec_formula(root, "work", "bead = { type = \"bead\", required = true }", "\"true\"");
        tracker(root);
        br(root, &["create", "Seed", "--silent"]);
    })
    .await;
    // The first read of a tracker is its history, not news: plan only after it (the seed turns ready).
    d.journal_until(WITHIN, |seen| seen.iter().any(|(k, _)| k == "bead.ready"))
        .await;
    let planner = d.create("plan").await;
    let count = |seen: &[(String, Value)], kind: &str| seen.iter().filter(|(k, _)| k == kind).count();
    // A bead is closed in br after its run passed (an effect of the terminal transaction).
    let journal = d
        .journal_until(WITHIN, |seen| {
            count(seen, "run.passed") == 4 && count(seen, "bead.closed") == 3
        })
        .await;

    let correlation = journal
        .iter()
        .find(|(_, e)| e["run"] == planner.as_str())
        .expect("the planner")
        .1["correlation"]
        .clone();
    assert!(correlation.is_string(), "{journal:?}");
    let runs: Vec<&Value> = journal
        .iter()
        .filter(|(k, _)| k == "run.passed")
        .map(|(_, e)| e)
        .collect();
    for run in &runs {
        assert_eq!(run["correlation"], correlation, "one chain: {run}");
    }
    let mut beads = HashSet::new();
    for run in runs.iter().filter(|e| e["run"] != planner.as_str()) {
        let view = d.get(&format!("/v1/runs/{}", run["run"].as_str().unwrap())).await;
        assert_eq!(view["formula"], "shop.work", "{view}");
        assert!(
            beads.insert(view["bead"].as_str().expect("a bead").to_owned()),
            "a bead taken twice: {view}"
        );
    }
    let listed: Value = serde_json::from_str(&br(d.dir.path(), &["list", "--all", "--json", "-l", "auto"])).unwrap();
    let epic = listed["issues"].as_array().expect("issues");
    assert_eq!(epic.len(), 3, "{listed}");
    for bead in epic {
        assert_eq!(bead["status"], "closed", "{bead}");
        assert!(
            beads.contains(bead["id"].as_str().unwrap()),
            "{bead} closed by a run of the chain"
        );
    }
    format!("4 runs in one correlation {correlation}")
}

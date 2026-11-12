//! Criterion 16 on the real br: the daemon claims a run's bead and holds the claim through the death of the step
//! (a `kill -9` of the daemon mid-step) and its resumed attempt; `passed` closes the bead, `failed` returns it; five
//! requests at once for the next ready bead of an epic of three give each bead to exactly one run; a bead whose runs
//! failed `bead_failures` times is no longer given out; every step gets the tracker's path in `BEADS_DIR`; br holds
//! no record of aae's own beyond claims and closes.

use super::{br, exec_formula, tracker};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

/// The bead `id` as br shows it.
fn show(root: &Path, id: &str) -> Value {
    let shown: Value = serde_json::from_str(&br(root, &["show", id, "--json"])).unwrap();
    shown[0].clone()
}

/// Waits until br shows the bead `id` with `status` and `assignee`: br is written after the run's transaction.
async fn until_shown(root: &Path, id: &str, status: &str, assignee: &Value) {
    let deadline = std::time::Instant::now() + WITHIN;
    loop {
        let shown = show(root, id);
        if shown["status"] == status && shown["assignee"] == *assignee {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{id} is not {status} by {assignee}: {shown}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Starts `formula` on the bead input `bead`; the reply.
async fn start(d: &Daemon, formula: &str, bead: Value) -> Value {
    let body = json!({ "project": "shop", "rig": "web", "formula": formula, "inputs": { "bead": bead } });
    d.post("/v1/runs", body).await.unwrap()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let mut beads: Vec<String> = Vec::new();
    let mut d = Daemon::start(&env, |root| {
        write(
            root.join("config/aae/config.toml"),
            "projects = [\"shop\"]\nbead_failures = 2\n",
        );
        let input = "bead = { type = \"bead\", required = true }";
        exec_formula(
            root,
            "hold",
            input,
            "\"sh\", \"-c\", \"printf %s \\\"$BEADS_DIR\\\"; sleep 1\"",
        );
        exec_formula(root, "break", input, "\"sh\", \"-c\", \"exit 3\"");
        tracker(root);
        beads.push(br(root, &["create", "Alone", "--silent"]));
        let many = br(root, &["create", "Many", "-t", "epic", "--silent"]);
        for n in 1..=3 {
            beads.push(br(
                root,
                &["create", &format!("Part {n}"), "--parent", &many, "--silent"],
            ));
        }
        let once = br(root, &["create", "Once", "-t", "epic", "--silent"]);
        beads.push(br(root, &["create", "Flaky", "--parent", &once, "--silent"]));
        beads.extend([many, once]);
    })
    .await;
    let (alone, parts, flaky, many, once) = (&beads[0], &beads[1..4], &beads[4], &beads[5], &beads[6]);
    let root = d.dir.path().to_owned();
    let tracker_dir = root.join("config/aae/shop/.beads");
    // The first read of the tracker fills the ready set the daemon picks from.
    let leaves: HashSet<&str> = beads[..5].iter().map(String::as_str).collect();
    let ready = |seen: &[(String, Value)]| {
        let ids = seen.iter().filter(|(k, _)| k == "bead.ready");
        ids.filter_map(|(_, e)| e["data"]["bead"]["id"].as_str())
            .filter(|id| leaves.contains(id))
            .count()
    };
    d.journal_until(WITHIN, |seen| ready(seen) == leaves.len()).await;

    // The claim through the death of the step and the resumed attempt; `passed` closes.
    let held = start(&d, "hold", json!(alone)).await["run"]
        .as_str()
        .unwrap()
        .to_owned();
    d.until_state(&held, "running", WITHIN).await;
    let owner = format!("aae:{held}");
    assert_eq!(
        show(&root, alone)["assignee"],
        owner.as_str(),
        "claimed as the run prepares"
    );
    d.restart().await;
    assert_eq!(
        show(&root, alone)["assignee"],
        owner.as_str(),
        "held through the restart"
    );
    let view = d.until_state(&held, "passed", WITHIN).await;
    assert_eq!(
        view["attempts"].as_array().map(Vec::len),
        Some(2),
        "resumed after the crash: {view}"
    );
    until_shown(&root, alone, "closed", &json!(owner)).await;

    // Five requests at once for the next ready bead of an epic of three.
    let next = json!({ "next_ready": true, "epic": many });
    let replies = futures_join(&d, &next).await;
    let runs: Vec<&str> = replies.iter().filter_map(|r| r["run"].as_str()).collect();
    let nothing = replies.iter().filter(|r| r["nothing_ready"] == true).count();
    assert_eq!((runs.len(), nothing), (3, 2), "{replies:?}");
    let mut given = HashSet::new();
    for run in &runs {
        let view = d.until_state(run, "passed", WITHIN).await;
        assert!(
            given.insert(view["bead"].as_str().unwrap().to_owned()),
            "a bead given twice: {view}"
        );
    }
    assert_eq!(
        given,
        parts.iter().cloned().collect(),
        "each bead of the epic to one run"
    );

    // `failed` returns the bead, and after `bead_failures` failures it is no longer given out.
    for n in 1..=2 {
        let run = start(&d, "break", json!({ "next_ready": true, "epic": once })).await;
        let run = run["run"]
            .as_str()
            .unwrap_or_else(|| panic!("failure {n}: {run}"))
            .to_owned();
        d.until_state(&run, "failed", WITHIN).await;
        until_shown(&root, flaky, "open", &Value::Null).await;
        // Given out again only once the daemon has read it back into the ready set.
        let again = |seen: &[(String, Value)]| {
            let ready = seen
                .iter()
                .filter(|(k, e)| k == "bead.ready" && e["data"]["bead"]["id"] == flaky.as_str());
            ready.count() > n
        };
        d.journal_until(WITHIN, again).await;
    }
    let refused = start(&d, "break", json!({ "next_ready": true, "epic": once })).await;
    assert_eq!(refused["nothing_ready"], true, "{refused}");

    // Every step had the tracker's path; br holds nothing of aae's but claims and closes.
    let journal = d
        .journal_until(WITHIN, |seen| {
            seen.iter().filter(|(k, _)| k == "run.passed").count() >= 4
        })
        .await;
    for (_, e) in journal.iter().filter(|(k, _)| k == "run.passed") {
        let stdout = &e["data"]["outcome"]["data"]["stdout"];
        assert_eq!(stdout.as_str().map(Path::new), Some(tracker_dir.as_path()), "{e}");
    }
    let listed: Value = serde_json::from_str(&br(&root, &["list", "--all", "--json"])).unwrap();
    let all = listed["issues"].as_array().expect("issues");
    assert_eq!(all.len(), beads.len(), "no bead of aae's own: {listed}");
    for bead in all.iter().map(|b| show(&root, b["id"].as_str().unwrap())) {
        assert_eq!(bead["created_by"], "alice", "{bead}");
        let labels = bead["labels"].as_array().map_or(0, Vec::len);
        let comments = bead["comments"].as_array().map_or(0, Vec::len);
        assert_eq!((labels, comments), (0, 0), "no label or comment of aae's: {bead}");
    }
    "claim held through a crash; 5 requests for an epic of 3: 3 runs, 2 nothing_ready; out after 2 failures".into()
}

/// Five requests for `next` at once.
async fn futures_join(d: &Daemon, next: &Value) -> Vec<Value> {
    let one = || start(d, "hold", next.clone());
    let (a, b, c, e, f) = tokio::join!(one(), one(), one(), one(), one());
    vec![a, b, c, e, f]
}

//! The br tracker of a project on a real `aae daemon` and the pinned br (spec §10.2, §10.3; §15 criteria 11, 16).

mod common;

use common::Daemon;
use serde_json::{Value, json};
use std::{path::Path, process::Command, time::Duration};

/// br by hand in the project, as `alice`; its trimmed stdout.
fn br(project: &Path, args: &[&str]) -> String {
    let out = Command::new("br")
        .args(args)
        .args(["--actor", "alice"])
        .env("BEADS_DIR", project.join(".beads"))
        .env("RUST_LOG", "error")
        .current_dir(project)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "br {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Reads the stream on to the next event of `kind` whose bead is `id`.
async fn until_event(events: &mut aae_front::client::Events, kind: &str, id: &str) -> Value {
    loop {
        let next = tokio::time::timeout(Duration::from_secs(20), events.next()).await;
        let event = next.unwrap_or_else(|_| panic!("no {kind} for {id}")).unwrap().unwrap();
        let envelope: Value = serde_json::from_str(&event.data).unwrap();
        if event.event == kind && envelope["data"]["bead"]["id"] == id {
            return envelope;
        }
    }
}

#[tokio::test]
async fn a_bead_written_by_hand_is_seen_and_a_run_on_it_claims_and_closes_it_in_br() {
    let fix = "name = \"fix\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
               [step]\nkind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n";
    let seed = std::cell::RefCell::new(String::new());
    let d = Daemon::start(|dir| {
        let project = dir.join("config/aae/shop");
        std::fs::write(project.join("formulas/fix.toml"), fix).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&project)
                .status()
                .unwrap()
                .success()
        );
        br(&project, &["init", "--prefix", "t", "-q"]);
        *seed.borrow_mut() = br(&project, &["create", "Seed", "-l", "rig:api", "--silent"]);
    })
    .await;
    let project = d.dir.path().join("config/aae/shop");
    let mut events = d.client.events(Some(1)).await.unwrap();
    // The first read of a tracker is its history, not news: write by hand only after it (the seed turns ready).
    until_event(&mut events, "bead.ready", &seed.borrow()).await;
    let id = br(&project, &["create", "Fix the cart", "-l", "rig:web", "--silent"]);
    let created = until_event(&mut events, "bead.created", &id).await;
    assert_eq!(
        (&created["data"]["actor"], &created["rig"]),
        (&json!("alice"), &json!("web"))
    );
    // The read that saw the write has the bead in the ready set, whichever of the two came first.
    let body =
        json!({ "project": "shop", "rig": "web", "formula": "fix", "inputs": { "bead": { "next_ready": true } } });
    let run = d.post("/v1/runs", body, None).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let view = d.until_state(&run, "passed").await;
    assert_eq!(view["bead"], json!(id));
    let closed = until_event(&mut events, "bead.closed", &id).await;
    assert_eq!(
        (&closed["run"], &closed["data"]["actor"]),
        (&json!(run), &json!(format!("aae:{run}")))
    );
    let shown: Value = serde_json::from_str(&br(&project, &["show", &id, "--json"])).unwrap();
    assert_eq!(
        (&shown[0]["status"], &shown[0]["assignee"]),
        (&json!("closed"), &json!(format!("aae:{run}")))
    );
}

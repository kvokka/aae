//! The br tracker of a project on a real `aae daemon` and the pinned br (docs/design/tracker-br.md; criteria 11, 16).

mod common;

use common::Daemon;
use serde_json::{Value, json};
use std::{path::Path, process::Command, time::Duration};

/// br by hand in the project, as `alice`; its trimmed stdout.
fn br(project: &Path, args: &[&str]) -> String {
    br_in(project, None, args)
}

/// br by hand, filing new beads with the id prefix `prefix` (a rig's) if any.
fn br_in(project: &Path, prefix: Option<&str>, args: &[&str]) -> String {
    let mut br = Command::new("br");
    if let Some(prefix) = prefix {
        br.env("BD_ISSUE_PREFIX", prefix);
    }
    let out = br
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
    let fix = "name = \"fix\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
               [[step]]\nkind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n";
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
        // A second rig, `api`, whose bead the run on `web` passes over.
        std::fs::write(
            project.join("aae.toml"),
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        )
        .unwrap();
        std::fs::create_dir(project.join("api")).unwrap();
        br(&project, &["init", "--prefix", "t", "-q"]);
        *seed.borrow_mut() = br_in(&project, Some("a"), &["create", "Seed", "--silent"]);
    })
    .await;
    let project = d.dir.path().join("config/aae/shop");
    // The bv workspace next to the orchestrator config lists the project's tracker with br's id prefix.
    let workspace = std::fs::read_to_string(d.dir.path().join("config/aae/.bv/workspace.yaml")).unwrap();
    let repo = format!("  - name: \"shop\"\n    path: {}\n    prefix: \"t-\"\n", json!(project));
    assert!(workspace.contains(&repo), "{workspace}");
    let mut events = d.client.events(Some(1)).await.unwrap();
    // The first read of a tracker is its history, not news: write by hand only after it (the seed turns ready).
    until_event(&mut events, "bead.ready", &seed.into_inner()).await;
    let id = br_in(&project, Some("w"), &["create", "Fix the cart", "--silent"]);
    assert!(id.starts_with("w-"), "{id}");
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
    // Nothing is ready now: the CLI says so and exits 3.
    let out = Command::new(env!("CARGO_BIN_EXE_aae"))
        .args([
            "run",
            "start",
            "shop",
            "web",
            "fix",
            "--input",
            r#"bead={"next_ready":true}"#,
        ])
        .env("AAE_SOCKET", d.socket())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        (out.status.code(), out.stdout.as_slice()),
        (Some(3), &b""[..]),
        "{said}"
    );
    assert!(said.contains("nothing ready"), "{said}");
}

#[tokio::test]
async fn a_tracker_that_cannot_be_watched_says_so_in_an_event() {
    use std::os::unix::fs::PermissionsExt;
    let d = Daemon::start(|dir| {
        let beads = dir.join("config/aae/shop/.beads");
        std::fs::create_dir_all(&beads).unwrap();
        // A directory with no read right cannot be watched.
        std::fs::set_permissions(&beads, std::fs::Permissions::from_mode(0o000)).unwrap();
    })
    .await;
    let journal = d.dir.path().join("state/aae/journal/events.jsonl");
    let deadline = std::time::Instant::now() + common::WAIT;
    let failed = loop {
        let text = std::fs::read_to_string(&journal).unwrap_or_default();
        let found = text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|e| e["type"] == "tracker.sync_failed" && e["data"]["op"] == "watch");
        if let Some(event) = found {
            break event;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no tracker.sync_failed{{op: watch}}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(failed["project"], "shop", "{failed}");
    let beads = d.dir.path().join("config/aae/shop/.beads");
    std::fs::set_permissions(beads, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A task names a bead filed a moment ago, before any read saw it: the run takes it up on its rig and files nothing;
/// a text becomes one root epic (docs/design/tracker-br.md; criterion 28).
#[tokio::test]
async fn a_task_takes_up_the_bead_it_names_and_a_text_files_one_root() {
    let fix = "name = \"fix\"\n[inputs]\ntask = { type = \"task\", required = true }\n\
               [[step]]\nkind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n";
    let d = Daemon::start(|dir| {
        let project = dir.join("config/aae/shop");
        std::fs::write(project.join("formulas/fix.toml"), fix).unwrap();
        let git = Command::new("git").args(["init", "-q"]).current_dir(&project).status();
        assert!(git.unwrap().success());
        br(&project, &["init", "--prefix", "t", "-q"]);
    })
    .await;
    let project = d.dir.path().join("config/aae/shop");
    let count = || {
        let listed: Value = serde_json::from_str(&br(&project, &["list", "-a", "--limit", "0", "--json"])).unwrap();
        listed["issues"].as_array().unwrap().clone()
    };
    let id = br_in(&project, Some("w"), &["create", "Fix the cart", "--silent"]);
    let start = |task: &str| json!({ "project": "shop", "formula": "fix", "inputs": { "task": task } });
    let run = d.post("/v1/runs", start(&id), None).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let view = d.until_state(&run, "passed").await;
    assert_eq!((&view["bead"], &view["rig"]), (&json!(id), &json!("web")));
    assert_eq!(count().len(), 1, "no second bead");
    let run = d.post("/v1/runs", start("Add a dark mode"), None).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let view = d.until_state(&run, "passed").await;
    let beads = count();
    let root = beads.iter().find(|b| b["id"] == view["bead"]).expect("the root bead");
    assert_eq!(
        (beads.len(), &root["title"], &root["issue_type"]),
        (2, &json!("Add a dark mode"), &json!("epic"))
    );
}

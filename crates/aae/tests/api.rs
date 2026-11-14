//! The HTTP API on a real `aae daemon` (spec §12, §13).

mod common;

use aae_front::client::{ClientError, SseEvent};
use common::{Daemon, WAIT, command, spawn};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{fs, time::Duration};

async fn next_event(events: &mut aae_front::client::Events) -> SseEvent {
    let event = tokio::time::timeout(WAIT, events.next()).await;
    event.expect("an event in time").unwrap().expect("the stream goes on")
}

#[tokio::test]
async fn a_run_started_over_the_api_passes_and_its_events_stream_from_a_seq() {
    let d = Daemon::start(|_| {}).await;
    let mut events = d.client.events(Some(1)).await.unwrap();
    let run = d.create("check", Some("k1")).await;
    assert_eq!(
        d.create("check", Some("k1")).await,
        run,
        "a repeated key is the same run"
    );
    assert_eq!(d.get("/v1/runs").await.as_array().unwrap().len(), 1);
    let view = d.until_state(&run, "passed").await;
    assert_eq!((&view["project"], &view["step_kind"]), (&json!("shop"), &json!("exec")));
    let passed = loop {
        let event = next_event(&mut events).await;
        let envelope: Value = serde_json::from_str(&event.data).unwrap();
        assert_eq!(event.id.as_deref(), Some(envelope["seq"].to_string().as_str()));
        if event.event == "run.passed" {
            assert_eq!(envelope["run"], run.as_str());
            break envelope["seq"].as_i64().unwrap();
        }
    };
    let mut again = d.client.events(Some(passed)).await.unwrap();
    assert_eq!(
        next_event(&mut again).await.event,
        "run.passed",
        "the stream continues from the seq"
    );
    assert_eq!(d.get("/v1/runs?state=passed&project=shop").await[0]["id"], run.as_str());
    // The live view of an ended run from after its end has nothing to send and closes by itself.
    let after = d.get("/v1/daemon").await["journal_seq"].as_i64().unwrap() + 1;
    let mut live = d
        .client
        .stream(&format!("/v1/runs/{run}/live?from={after}"))
        .await
        .unwrap();
    let end = tokio::time::timeout(WAIT, live.next()).await;
    assert!(end.expect("the view closes").unwrap().is_none());
}

#[tokio::test]
async fn an_operator_gate_holds_a_new_run_until_opened_by_hand() {
    let d = Daemon::start(|_| {}).await;
    let gate = json!({ "key": "project:shop", "reason": "maintenance" });
    d.post("/v1/gates", gate, None).await.unwrap();
    let gates = d.get("/v1/gates").await;
    assert_eq!(
        (&gates[0]["key"], &gates[0]["closed_by"]),
        (&json!("project:shop"), &json!("operator"))
    );
    // The run is queued for the gate in the transaction that creates it, so nothing starts it until the gate opens.
    let run = d.create("check", None).await;
    let view = d.get(&format!("/v1/runs/{run}")).await;
    assert_eq!(
        (&view["state"], &view["waiting_for"]),
        (&json!("queued"), &json!("gate")),
        "{view}"
    );
    let past = json!({ "key": "rig:shop/web", "until": "2000-01-01T00:00:00Z" });
    match d.post("/v1/gates", past, None).await {
        Err(ClientError::Api { status: 409, error }) => assert_eq!(error["code"], "rejected"),
        other => panic!("{other:?}"),
    }
    d.post("/v1/gates/open", json!({ "key": "project:shop" }), None)
        .await
        .unwrap();
    d.until_state(&run, "passed").await;
    assert_eq!(d.get("/v1/gates").await, json!([]));
}

#[tokio::test]
async fn a_run_is_cancelled_and_bad_requests_get_json_errors() {
    let d = Daemon::start(|_| {}).await;
    let run = d.create("slow", None).await;
    d.until_state(&run, "running").await;
    d.post(&format!("/v1/runs/{run}/cancel"), json!({}), None)
        .await
        .unwrap();
    d.until_state(&run, "cancelled").await;
    match d.client.request("GET", "/v1/runs/nope", None, None).await {
        Err(ClientError::Api { status: 404, error }) => assert_eq!(error["code"], "not_found"),
        other => panic!("{other:?}"),
    }
    match d.post("/v1/runs", json!({ "project": "shop" }), None).await {
        Err(ClientError::Api { status: 400, error }) => assert_eq!(error["code"], "bad_request"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_rotated_away_seq_is_an_explicit_gap() {
    // The journal as rotation leaves it: its oldest event is 5.
    let d = Daemon::start(|dir| {
        let journal = dir.join("state/aae/journal");
        fs::create_dir_all(&journal).unwrap();
        fs::write(journal.join("events.jsonl"), "{\"seq\":5,\"type\":\"run.queued\"}\n").unwrap();
    })
    .await;
    match d.client.events(Some(2)).await {
        Err(ClientError::Api { status: 410, error }) => {
            assert_eq!((&error["code"], &error["oldest"]), (&json!("gap"), &json!(5)));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("no gap"),
    }
    // The live view of a run likewise, before its stream starts.
    let run = d.create("check", None).await;
    match d.client.stream(&format!("/v1/runs/{run}/live?from=2")).await {
        Err(ClientError::Api { status: 410, error }) => assert_eq!(error["oldest"], 5),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("no gap"),
    }
}

#[tokio::test]
async fn one_daemon_per_socket_and_a_stop_signal_removes_the_socket() {
    let mut d = Daemon::start(|_| {}).await;
    let second = spawn(d.dir.path()).wait_with_output().unwrap();
    assert!(
        !second.status.success(),
        "a second daemon must not start on a live socket"
    );
    assert!(d.terminate().success());
    assert!(!d.socket().exists());
}

#[tokio::test]
async fn one_daemon_per_state_directory_whatever_its_socket() {
    let d = Daemon::start(|_| {}).await;
    let state = d.dir.path().join("state/aae");
    // The journal writer creates its file after the API answers.
    let deadline = std::time::Instant::now() + WAIT;
    while journal_lines(&d, "daemon.started").is_empty() {
        assert!(std::time::Instant::now() < deadline, "no daemon.started in the journal");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for file in [
        d.socket(),
        state.join("state.db"),
        state.join("state.db-wal"),
        state.join("journal/events.jsonl"),
    ] {
        let mode = fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{}", file.display());
    }
    let second = command(d.dir.path())
        .env("XDG_RUNTIME_DIR", d.dir.path().join("other-run"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success(), "a second daemon must not share the state");
    let state = d.dir.path().join("state/aae");
    assert!(
        stderr.contains(&format!("another daemon owns {}", state.display())),
        "{stderr}"
    );
}

#[tokio::test]
async fn a_trigger_fired_by_hand_starts_a_run_skips_while_it_lives_and_switches_off() {
    let d = Daemon::start(|dir| {
        let project = "[rigs.web]\npath = \"web\"\n\
            [[trigger]]\nname = \"sweep\"\nevery = \"24h\"\nrun = \"slow\"\nrig = \"web\"\n";
        fs::write(dir.join("config/aae/shop/aae.toml"), project).unwrap();
    })
    .await;
    let fire = json!({ "project": "shop", "rig": "web", "trigger": "sweep" });
    let run = d.post("/v1/triggers/fire", fire.clone(), Some("f1")).await.unwrap()["run"].clone();
    d.until_state(run.as_str().unwrap(), "running").await;
    let again = d.post("/v1/triggers/fire", fire.clone(), Some("f2")).await.unwrap();
    assert_eq!(again["skipped"], "overlap");
    assert_eq!(
        d.post("/v1/triggers/fire", fire.clone(), Some("f1")).await.unwrap()["run"],
        run
    );
    let unknown = json!({ "project": "shop", "rig": "web", "trigger": "nope" });
    match d.post("/v1/triggers/fire", unknown, None).await {
        Err(ClientError::Api { status: 409, error }) => assert_eq!(error["code"], "rejected"),
        other => panic!("{other:?}"),
    }
    let off = d.post("/v1/triggers/disable", fire.clone(), None).await.unwrap();
    assert_eq!(off, json!({ "trigger": "shop.sweep", "enabled": false }));
    assert_eq!(d.get("/v1/triggers").await[0]["enabled"], false);
    d.post("/v1/triggers/enable", fire, None).await.unwrap();
    assert_eq!(d.get("/v1/triggers").await[0]["enabled"], true);
}

/// A replayed reload answers as the first one did and leaves the config the daemon runs with alone.
#[tokio::test]
async fn a_replayed_reload_keeps_the_config_it_applied() {
    let d = Daemon::start(|_| {}).await;
    let reload = d.post("/v1/config/reload", json!({}), Some("r1")).await.unwrap();
    assert_eq!(reload["config_generation"], 2);
    let project = "[rigs.web]\npath = \"web\"\n\
        [[trigger]]\nname = \"sweep\"\nevery = \"24h\"\nrun = \"slow\"\nrig = \"web\"\n";
    fs::write(d.dir.path().join("config/aae/shop/aae.toml"), project).unwrap();
    let replay = d.post("/v1/config/reload", json!({}), Some("r1")).await.unwrap();
    assert_eq!(replay, reload);
    let triggers = d.get("/v1/triggers").await;
    assert_eq!(triggers, json!([]), "the file read by the replay is not applied");
    let fresh = d.post("/v1/config/reload", json!({}), Some("r2")).await.unwrap();
    assert_eq!(fresh["config_generation"], 3);
    assert_eq!(d.get("/v1/triggers").await[0]["trigger"], "shop.sweep");
}

/// The journal lines of `type` in the daemon's journal file.
fn journal_lines(d: &Daemon, kind: &str) -> Vec<Value> {
    // No file yet: the writer has not drained its first commit.
    let text = fs::read_to_string(d.dir.path().join("state/aae/journal/events.jsonl")).unwrap_or_default();
    // A line the daemon is still appending has no newline yet.
    let whole = text.split_inclusive('\n').filter(|l| l.ends_with('\n'));
    let lines = whole.map(|l| serde_json::from_str::<Value>(l).unwrap());
    lines.filter(|e| e["type"] == kind).collect()
}

#[tokio::test]
async fn a_drain_starts_no_run_and_stops_the_daemon_when_its_timeout_cuts_a_turn() {
    let mut d = Daemon::start(|_| {}).await;
    let slow = d.create("slow", None).await;
    d.until_state(&slow, "running").await;
    let status = d.get("/v1/daemon").await;
    assert_eq!(
        (
            &status["active_runs"],
            &status["journal"]["failures"],
            &status["stopping"]
        ),
        (&json!(1), &json!(0), &Value::Null),
        "{status}"
    );
    assert!(status["outbox_depth"].is_i64(), "{status}");

    let drained = d.post("/v1/daemon/drain?timeout_s=2", json!(null), None).await.unwrap();
    assert_eq!(drained["active_runs"], 1, "{drained}");
    match d.post("/v1/daemon/restart", json!(null), None).await {
        Err(ClientError::Api { status: 409, .. }) => {}
        other => panic!("a restart during a drain: {other:?}"),
    }
    let held = d.create("check", None).await;
    let view = d.until_state(&held, "queued").await;
    assert_eq!(view["waiting_for"], "daemon_stopping", "{view}");

    let deadline = std::time::Instant::now() + WAIT;
    let exit = loop {
        if let Some(exit) = d.child.try_wait().unwrap() {
            break exit;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the drain did not stop the daemon"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(exit.success() && !d.socket().exists());
    let stopping = journal_lines(&d, "daemon.stopping");
    assert_eq!(stopping.len(), 1, "{stopping:?}");
    assert_eq!(stopping[0]["data"]["reason"], "drain");
}

#[tokio::test]
async fn a_restart_starts_the_daemon_again_in_the_same_process() {
    let d = Daemon::start(|dir| {
        // A binary of its own, to replace on disk before the restart, as an upgrade does.
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_aae"), dir.join("bin/aae")).unwrap();
        let brief = "name = \"brief\"\n[step]\nkind = \"exec\"\ncommand = [\"sleep\", \"1\"]\ntimeout = \"10s\"\n";
        fs::write(dir.join("config/aae/shop/formulas/brief.toml"), brief).unwrap();
    })
    .await;
    assert_eq!(
        d.get("/v1/instances").await,
        json!([]),
        "exec formulas start no instance"
    );
    match d.post("/v1/instances/nope/stop", json!(null), None).await {
        Err(ClientError::Api { status: 409, .. }) => {}
        other => panic!("{other:?}"),
    }
    let before = d.get("/v1/daemon").await;
    let brief = d.create("brief", None).await;
    d.until_state(&brief, "running").await;
    let bin = d.dir.path().join("bin");
    fs::copy(env!("CARGO_BIN_EXE_aae"), bin.join("aae.new")).unwrap();
    fs::rename(bin.join("aae.new"), bin.join("aae")).unwrap();
    d.post("/v1/daemon/restart?timeout_s=30", json!(null), None)
        .await
        .unwrap();
    // Queued during the drain: the restarted daemon starts it with nothing else touching the queue.
    let held = d.create("check", None).await;
    let deadline = std::time::Instant::now() + WAIT;
    let after = loop {
        if let Ok(after) = d.client.request("GET", "/v1/daemon", None, None).await
            && after["started_at"] != before["started_at"]
            // The journal writes `daemon.started` after the API answers.
            && journal_lines(&d, "daemon.started").len() == 2
        {
            break after;
        }
        assert!(std::time::Instant::now() < deadline, "the daemon did not come back");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(after["pid"], before["pid"], "{after}");
    #[cfg(target_os = "linux")]
    assert_eq!(
        fs::read_link(format!("/proc/{}/exe", after["pid"])).unwrap(),
        bin.join("aae"),
        "the daemon runs the new binary"
    );
    assert_eq!(journal_lines(&d, "daemon.stopping")[0]["data"]["reason"], "restart");
    // From the journal, not the live view: the restarted daemon may have started the run already.
    let queued = journal_lines(&d, "run.queued");
    let first = queued.iter().find(|e| e["run"] == held.as_str()).unwrap();
    assert_eq!(first["data"]["waiting_for"], "daemon_stopping", "{first}");
    d.until_state(&held, "passed").await;
    d.until_state(&brief, "passed").await;
    assert_eq!(
        journal_lines(&d, "run.queued").len(),
        2,
        "no run is created by the restart"
    );
    let run = d.create("check", None).await;
    d.until_state(&run, "passed").await;
}

#[tokio::test]
async fn the_daemon_exits_with_an_error_once_its_core_stops() {
    let mut d = Daemon::start(|dir| {
        let long = "name = \"long\"\n[step]\nkind = \"exec\"\ncommand = [\"sleep\", \"30\"]\ntimeout = \"1s\"\n";
        fs::write(dir.join("config/aae/shop/formulas/long.toml"), long).unwrap();
    })
    .await;
    let run = d.create("long", None).await;
    d.until_state(&run, "running").await;
    // Another writer holds the database: the step's deadline cannot be written, and the core stops on the error.
    let db = rusqlite::Connection::open(d.dir.path().join("state/aae/state.db")).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = d.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the daemon lives on without its core"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(!status.success(), "{status}");
}

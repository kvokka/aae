//! The HTTP API on a real `aae daemon` (spec §12, §13): exec formulas, so no harness is needed.

use aae_front::client::{Client, ClientError, SseEvent};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

const FILES: &[(&str, &str)] = &[
    ("config/aae/config.toml", "projects = [\"shop\"]\n"),
    ("config/aae/shop/aae.toml", "[rigs.web]\npath = \"web\"\n"),
    ("config/aae/shop/web/.keep", ""),
    (
        "config/aae/shop/formulas/check.toml",
        "name = \"check\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
    ),
    (
        "config/aae/shop/formulas/slow.toml",
        "name = \"slow\"\n[step]\nkind = \"exec\"\ncommand = [\"sleep\", \"30\"]\ntimeout = \"60s\"\n",
    ),
];

struct Daemon {
    child: Child,
    dir: tempfile::TempDir,
    client: Client,
}

impl Daemon {
    fn socket(&self) -> PathBuf {
        self.dir.path().join("run/aae/aae.sock")
    }

    /// `aae daemon` with the XDG directories in a temporary one; `before` prepares it.
    async fn start(before: impl FnOnce(&std::path::Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        before(dir.path());
        let child = spawn(dir.path());
        let client = Client::new(dir.path().join("run/aae/aae.sock"));
        let daemon = Self { child, dir, client };
        let deadline = Instant::now() + Duration::from_secs(10);
        while daemon.client.request("GET", "/v1/daemon", None, None).await.is_err() {
            assert!(Instant::now() < deadline, "the daemon did not come up");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        daemon
    }

    async fn get(&self, path: &str) -> Value {
        self.client.request("GET", path, None, None).await.unwrap()
    }

    async fn post(&self, path: &str, body: Value, key: Option<&str>) -> Result<Value, ClientError> {
        self.client.request("POST", path, Some(&body), key).await
    }

    async fn create(&self, formula: &str, key: Option<&str>) -> String {
        let body = json!({ "project": "shop", "rig": "web", "formula": formula });
        let reply = self.post("/v1/runs", body, key).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// Waits until the run is in `state`.
    async fn until_state(&self, run: &str, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let view = self.get(&format!("/v1/runs/{run}")).await;
            if view["state"] == state {
                return view;
            }
            assert!(Instant::now() < deadline, "{run} stays {}", view["state"]);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn terminate(&mut self) -> ExitStatus {
        let pid = self.child.id().to_string();
        assert!(Command::new("kill").args(["-TERM", &pid]).status().unwrap().success());
        self.child.wait().unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_aae"))
        .arg("daemon")
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .spawn()
        .unwrap()
}

async fn next_event(events: &mut aae_front::client::Events) -> SseEvent {
    let event = tokio::time::timeout(Duration::from_secs(10), events.next()).await;
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
    let run = d.create("check", None).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(d.get(&format!("/v1/runs/{run}")).await["state"], "queued");
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

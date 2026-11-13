//! Services (spec §11.4, docs/design/contracts.md 8) in the core: one process per scope shared by the sessions that
//! name it, its URL with the token and the identity headers in each session, a restart on the same port after it
//! dies, a stop by the operator once it is idle.

mod common;

use aae_core::{
    Journal,
    run::{Msg, Msg::StopInstance},
};
use aae_testkit::ScriptedDriver;
use common::{World, world_driven};
use serde_json::Value;
use std::time::{Duration, Instant};

const PROBE: &str = env!("CARGO_BIN_EXE_mcp-probe");

impl World {
    fn service_events(&self) -> Vec<(String, Value)> {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let services = all
            .into_iter()
            .filter(|e| e["type"].as_str().unwrap().starts_with("service."));
        services
            .map(|e| (e["type"].as_str().unwrap().to_owned(), e["data"].clone()))
            .collect()
    }

    fn count(&self, kind: &str) -> usize {
        self.service_events().iter().filter(|(t, _)| t == kind).count()
    }

    /// Waits in real time (the service is a real process) until `kind` was emitted `n` times.
    async fn until(&self, kind: &str, n: usize) {
        let start = Instant::now();
        while self.count(kind) < n {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "no {kind} #{n}: {:?}",
                self.service_events()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The pids in the probe's log, in order.
fn pids(log: &std::path::Path) -> Vec<u64> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines = text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap());
    lines.filter_map(|l| l["pid"].as_u64()).collect()
}

/// A blocking `initialize` POST to the session's view of the server, as a harness would send it.
fn call(server: &Value) -> u16 {
    let url = server["url"].as_str().unwrap();
    let mut args = vec![
        "-s".to_owned(),
        "-o".to_owned(),
        "/dev/null".to_owned(),
        "-w".to_owned(),
        "%{http_code}".to_owned(),
    ];
    for (k, v) in server["headers"].as_object().unwrap() {
        args.extend(["-H".to_owned(), format!("{k}: {}", v.as_str().unwrap())]);
    }
    args.extend([
        "-H".to_owned(),
        "content-type: application/json".to_owned(),
        "-d".to_owned(),
    ]);
    args.push(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_owned());
    args.push(url.to_owned());
    let out = std::process::Command::new("curl").args(&args).output().unwrap();
    String::from_utf8_lossy(&out.stdout).parse().unwrap_or_default()
}

#[tokio::test(start_paused = true)]
async fn one_service_serves_every_session_and_restarts_on_its_port() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("probe.jsonl");
    let agent = format!(
        "harness = \"scripted\"\n[mcp.probe]\ncommand = [\"{PROBE}\", \"{}\"]\n\
         url = \"http://127.0.0.1:${{AAE_SERVICE_PORT}}/mcp\"\n",
        log.display()
    );
    let driver = ScriptedDriver::new(4);
    driver.stall_sessions();
    let rigs = "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n";
    let files = [
        ("shop/agents/dev/agent.toml", agent.as_str()),
        ("shop/aae.toml", rigs),
        ("shop/api/.keep", ""),
    ];
    let w = world_driven(&files, driver);
    let (a, b) = (w.create().await, w.create_on("api").await);
    w.until("service.ready", 1).await;
    // Both sessions are running before the service is counted.
    w.events(&a, Duration::from_millis(500)).await;
    assert_eq!(w.count("service.starting"), 1, "one process for both sessions");
    let (sa, sb) = (
        w.driver.mcp(&a, 1)["probe"].clone(),
        w.driver.mcp(&b, 1)["probe"].clone(),
    );
    assert_eq!(sa["url"], sb["url"], "the same server");
    assert!(!sa["url"].as_str().unwrap().contains("AAE_SERVICE_PORT"));
    assert_eq!(sa["headers"]["X-Aae-Run"], a.as_str());
    assert_eq!(sb["headers"]["X-Aae-Run"], b.as_str());
    assert_eq!(sa["headers"]["X-Aae-Agent"], "shop.dev");
    assert!(sa["command"].is_null(), "a service reaches the session as its URL only");
    assert_eq!(call(&sa), 200, "the session's token is the service's");
    let mut wrong = sa.clone();
    wrong["headers"]["Authorization"] = "Bearer nope".into();
    assert_eq!(call(&wrong), 401);

    // It dies: started again on the same port, so the sessions holding its URL reconnect.
    let pid = pids(&log)[0];
    std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    w.until("service.ready", 2).await;
    assert_eq!(w.count("service.failed"), 1);
    assert_eq!(call(&sa), 200, "the same URL works again");
    assert_ne!(*pids(&log).last().unwrap(), pid);

    // The operator stops it only once no session uses it.
    let id = w.service_events()[0].1["service"]["id"].as_str().unwrap().to_owned();
    assert!(
        w.core.send(None, StopInstance { id: id.clone() }).await.is_err(),
        "it has sessions"
    );
    for run in [&a, &b] {
        let cancel = Msg::Cancel {
            run: run.clone(),
            reason: "done".into(),
        };
        w.core.send(None, cancel).await.unwrap();
    }
    w.events(&a, Duration::from_millis(500)).await;
    w.core.send(None, StopInstance { id }).await.unwrap();
    w.until("service.stopped", 1).await;
    assert_ne!(call(&sa), 200, "the process is gone");
}

/// A process alive with `pid`.
fn alive(pid: u64) -> bool {
    let status = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status();
    status.unwrap().success()
}

#[tokio::test(start_paused = true)]
async fn a_service_that_never_comes_up_fails_the_tool_not_the_run() {
    let agent = "harness = \"scripted\"\n[mcp.probe]\ncommand = [\"false\"]\n\
                 url = \"http://127.0.0.1:${AAE_SERVICE_PORT}/mcp\"\n";
    let w = world_driven(&[("shop/agents/dev/agent.toml", agent)], ScriptedDriver::new(1));
    let run = w.create().await;
    w.until("service.failed", 1).await;
    // The launch waits for the port at most 30 s, then the session starts with the URL all the same.
    tokio::time::sleep(Duration::from_secs(31)).await;
    let states = w.states(&run).await;
    assert_eq!(states.last().unwrap(), "running", "{states:?}");
    assert!(w.driver.mcp(&run, 1)["probe"]["url"].is_string());
}

#[tokio::test(start_paused = true)]
async fn a_daemon_crash_stops_the_service_it_left_and_the_next_session_gets_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("probe.jsonl");
    let agent = format!(
        "harness = \"scripted\"\n[mcp.probe]\ncommand = [\"{PROBE}\", \"{}\"]\n\
         url = \"http://127.0.0.1:${{AAE_SERVICE_PORT}}/mcp\"\n",
        log.display()
    );
    let driver = ScriptedDriver::new(1);
    driver.stall_sessions();
    let mut w = world_driven(&[("shop/agents/dev/agent.toml", agent.as_str())], driver);
    let first = w.create().await;
    w.until("service.ready", 1).await;
    assert_eq!(call(&w.driver.mcp(&first, 1)["probe"]), 200);
    let pid = pids(&log)[0];
    w.restart().await;
    let start = Instant::now();
    while alive(pid) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the service of the dead daemon lives on"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        std::thread::sleep(Duration::from_millis(20));
    }
    let run = w.create().await;
    w.until("service.ready", 2).await;
    let server = w.driver.mcp(&run, 1)["probe"].clone();
    assert_eq!(call(&server), 200, "a new service for the new session");
    assert_ne!(*pids(&log).last().unwrap(), pid);
}

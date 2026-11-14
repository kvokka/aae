//! Helpers of the service tests: a project with a `[[service]]`, its events, a call as a session makes it.

use super::World;
use aae_core::Journal;
use serde_json::Value;
use std::time::{Duration, Instant};

pub(crate) const PROBE: &str = env!("CARGO_BIN_EXE_mcp-probe");

/// `shop/aae.toml` with rig `web` and `service`, a `[[service]]` table's keys.
pub(crate) fn project(service: &str) -> (&'static str, String) {
    (
        "shop/aae.toml",
        format!("[rigs.web]\npath = \"web\"\n[[service]]\n{service}\n"),
    )
}

impl World {
    pub(crate) fn service_events(&self) -> Vec<(String, Value)> {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let services = all
            .into_iter()
            .filter(|e| e["type"].as_str().unwrap().starts_with("service."));
        services
            .map(|e| (e["type"].as_str().unwrap().to_owned(), e["data"].clone()))
            .collect()
    }

    pub(crate) fn service_count(&self, kind: &str) -> usize {
        self.service_events().iter().filter(|(t, _)| t == kind).count()
    }

    /// The services started, restarts aside: another process may take a port between its pick and the service's
    /// bind, and the service starts again on another one (`Address already in use` under parallel tests).
    pub(crate) fn services(&self) -> usize {
        let starts = self
            .service_events()
            .into_iter()
            .filter(|(t, _)| t == "service.starting");
        let ids: std::collections::HashSet<_> = starts.map(|(_, d)| d["service"]["id"].clone()).collect();
        ids.len()
    }

    /// Waits in real time (the service is a real process) until `kind` was emitted `n` times.
    pub(crate) async fn until_count(&self, kind: &str, n: usize) {
        let start = Instant::now();
        while self.service_count(kind) < n {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "no {kind} #{n}: {:?}\nlisteners:\n{}",
                self.service_events(),
                listeners()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// What listens on TCP, with the processes: why a service did not pass its check.
fn listeners() -> String {
    let out = std::process::Command::new("ss").arg("-Htlnp").output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// A blocking `initialize` POST to the session's view of the server, as a harness would send it.
pub(crate) fn call(server: &Value) -> u16 {
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

/// Waits in real time for the file `path` to hold a line and returns it; the core runs meanwhile.
pub(crate) async fn file(path: &std::path::Path) -> String {
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.ends_with('\n') {
            return text;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "no {}", path.display());
        tokio::time::sleep(Duration::from_millis(20)).await;
        std::thread::sleep(Duration::from_millis(20));
    }
}

//! A service whose port someone else takes (docs/design/config.md): before it is ready it moves and the session
//! follows; once ready it moves after two starts in a row on the taken port, with a new token.

use crate::common;

use common::services::{PROBE, call, file, project};
use common::world_driven;
use loop_manager_testkit::ScriptedDriver;
use std::time::{Duration, Instant};

/// Lets the core run for `real` in real time.
async fn pass(real: Duration) {
    let start = Instant::now();
    while start.elapsed() < real {
        tokio::time::sleep(Duration::from_millis(20)).await;
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A service whose first start writes its port to `port`, waits for `go` (or the end of
/// the test), then dies without listening; later starts run `then` (`$1`, `$2`) or the same again. Every start adds its
/// port to `ports`.
fn first_dies(dir: &std::path::Path, then: &str, more: &str) -> (&'static str, String) {
    let script = "echo $LOOP_MANAGER_SERVICE_PORT >> \\\"$0/ports\\\"; if [ -n \\\"$1\\\" ] && [ -e \
    \\\"$0/once\\\" ]; then exec \\\"$1\\\" \\\"$2\\\"; fi; \
                  touch \\\"$0/once\\\"; echo $LOOP_MANAGER_SERVICE_PORT > \\\"$0/port\\\"; \
                  while [ -d \\\"$0\\\" ] && [ ! -e \\\"$0/go\\\" ]; do sleep 0.05; done; exit 1";
    let log = dir.join("probe.jsonl");
    project(&format!(
        "name = \"probe\"\ncommand = [\"sh\", \"-c\", \"{script}\", \"{}\", \"{then}\", \"{}\"]\nmcp = true\n{more}",
        dir.display(),
        log.display()
    ))
}

/// The scenario of the test below; `false` if another process listened on the port before the test could.
async fn taken_before_ready() -> bool {
    let dir = tempfile::tempdir().unwrap();
    let driver = ScriptedDriver::new(1);
    driver.stall_sessions();
    let (path, text) = first_dies(dir.path(), PROBE, "");
    let w = world_driven(&[(path, text.as_str())], driver);
    let run = w.create().await;
    let taken: u16 = file(&dir.path().join("port")).await.trim().parse().unwrap();
    let Ok(other) = std::net::TcpListener::bind(("127.0.0.1", taken)) else {
        return false;
    };
    other.set_nonblocking(true).unwrap();
    // Port checks run while the other process listens on the port: none passes.
    pass(Duration::from_millis(400)).await;
    assert_eq!(w.service_count("service.ready"), 0, "ready on someone else's port");
    std::fs::write(dir.path().join("go"), "").unwrap();
    w.until_count("service.ready", 1).await;
    w.events(&run, Duration::from_millis(500)).await;
    let server = w.driver.mcp(&run, 1)["probe"].clone();
    let url = server["url"].as_str().unwrap();
    assert!(!url.contains(&format!(":{taken}/")), "{url} is the taken port");
    assert_eq!(call(&server), 200, "the session reaches the service");
    assert_eq!(w.services(), 1);
    let accepted = other.accept().map(drop);
    assert_eq!(
        accepted.unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "the token went elsewhere"
    );
    true
}

#[tokio::test]
async fn a_service_whose_port_is_taken_before_it_is_ready_moves_and_the_session_follows() {
    // Under parallel tests another process may take the port before this one does: the scenario runs again.
    for _ in 0..3 {
        if taken_before_ready().await {
            return;
        }
    }
    panic!("another process took the port three times");
}

/// The scenario of the test below; `false` if another process listened on the port before the test could.
async fn taken_when_ready() -> bool {
    let dir = tempfile::tempdir().unwrap();
    let up = dir.path().join("up");
    let health = format!(
        "health = {{ command = [\"test\", \"-e\", \"{}\"] }}\n[rigs.api]\npath = \"api\"",
        up.display()
    );
    let (path, text) = first_dies(dir.path(), PROBE, &health);
    let driver = ScriptedDriver::new(1);
    driver.stall_sessions();
    let w = world_driven(&[(path, text.as_str()), ("shop/api/.keep", "")], driver);
    let first = w.create().await;
    let taken: u16 = file(&dir.path().join("port")).await.trim().parse().unwrap();
    std::fs::write(&up, "").unwrap();
    w.until_count("service.ready", 1).await;
    w.events(&first, Duration::from_millis(500)).await;
    let old = w.driver.mcp(&first, 1)["probe"]["headers"]["Authorization"].clone();
    // The service never listens: if another process holds the port now, the scenario runs again.
    let Ok(_other) = std::net::TcpListener::bind(("127.0.0.1", taken)) else {
        return false;
    };
    std::fs::write(dir.path().join("go"), "").unwrap();
    w.until_count("service.ready", 2).await;
    let failed = w.service_events().into_iter().filter(|(t, _)| t == "service.failed");
    let raw: Vec<String> = failed
        .map(|(_, d)| d["failure"]["raw"].as_str().unwrap().to_owned())
        .collect();
    let is_taken = format!("port {taken} is taken");
    assert_eq!(raw.iter().filter(|r| r.contains(&is_taken)).count(), 2, "{raw:?}");
    let ports = std::fs::read_to_string(dir.path().join("ports")).unwrap();
    let ports: Vec<&str> = ports.lines().collect();
    assert_eq!(ports.len(), 2, "a start on a taken port: {ports:?}");
    assert_ne!(ports[1], taken.to_string());
    // Who holds the old port may have the old token from the sessions holding the old URL: it is void.
    let next = w.create_on("api").await;
    w.events(&next, Duration::from_millis(500)).await;
    let mut server = w.driver.mcp(&next, 1)["probe"].clone();
    assert!(old.is_string());
    assert_ne!(server["headers"]["Authorization"], old);
    assert_eq!(call(&server), 200);
    server["headers"]["Authorization"] = old;
    assert_eq!(call(&server), 401, "the old token");
    true
}

#[tokio::test]
async fn a_ready_service_whose_port_is_taken_moves_to_a_new_one_with_a_new_token() {
    for _ in 0..3 {
        if taken_when_ready().await {
            return;
        }
    }
    panic!("another process took the port three times");
}

#[tokio::test]
async fn a_fixed_bind_conflict_stops_the_service_without_trying_another_port() {
    let dir = tempfile::tempdir().unwrap();
    let other = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = other.local_addr().unwrap().port();
    let (path, text) = first_dies(dir.path(), PROBE, &format!("bind = \"0.0.0.0:{port}\""));
    let w = world_driven(&[(path, text.as_str())], ScriptedDriver::new(1));
    w.create().await;
    w.until_count("service.stopped", 1).await;
    let events = w.service_events();
    assert_eq!(w.service_count("service.starting"), 1, "{events:?}");
    assert!(events.iter().any(|(kind, data)| {
        kind == "service.failed"
            && data["failure"]["raw"]
                .as_str()
                .unwrap()
                .contains(&format!("port {port} is taken"))
    }));
    assert!(!dir.path().join("once").exists(), "service command ran on a taken port");
}

#[tokio::test]
async fn a_fixed_service_retries_on_its_requested_port_before_readiness() {
    let dir = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (path, text) = first_dies(dir.path(), PROBE, &format!("bind = \"0.0.0.0:{port}\""));
    let driver = ScriptedDriver::new(1);
    driver.stall_sessions();
    let w = world_driven(&[(path, text.as_str())], driver);
    drop(listener);
    let run = w.create().await;
    assert_eq!(file(&dir.path().join("port")).await.trim(), port.to_string());
    std::fs::write(dir.path().join("go"), "").unwrap();
    w.until_count("service.ready", 1).await;
    w.events(&run, Duration::from_millis(500)).await;
    let server = w.driver.mcp(&run, 1)["probe"].clone();
    assert_eq!(server["url"], format!("http://127.0.0.1:{port}/mcp"));
    assert_eq!(call(&server), 200);
    w.core
        .send(
            None,
            loop_manager_core::run::Msg::Cancel {
                run,
                reason: "done".to_owned(),
            },
        )
        .await
        .unwrap();
    let id = w.service_events()[0].1["service"]["id"].as_str().unwrap().to_owned();
    w.core
        .send(None, loop_manager_core::run::Msg::StopInstance { id })
        .await
        .unwrap();
    w.until_count("service.stopped", 1).await;
}

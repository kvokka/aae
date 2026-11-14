//! Services (spec §11.4, docs/design/contracts.md 8) in the core: one `[[service]]` process per scope shared by the
//! sessions, its MCP URL with the token and the identity headers in each session (or its URL and token in their
//! environment), ready once its health check passes, a restart on the same port after it dies (on a new one until
//! it was first ready), a stop by the operator once it is idle. A port someone else takes: `service_ports.rs`.
//!
//! The tests with a live service run in real time: under a paused clock the runtime skips to the next timer while it
//! waits for a real process, and the 30 s a service has to come up pass in milliseconds.

mod common;

use aae_core::run::{Msg, Msg::StopInstance};
use aae_testkit::ScriptedDriver;
use common::services::{PROBE, call, file, project};
use common::world_driven;
use serde_json::Value;
use std::time::{Duration, Instant};

/// The probe as a service named `probe` serving MCP, logging to `log`.
fn probe(log: &std::path::Path, more: &str) -> (&'static str, String) {
    let command = format!(
        "name = \"probe\"\ncommand = [\"{PROBE}\", \"{}\"]\nmcp = true",
        log.display()
    );
    project(&format!("{command}\n{more}"))
}

/// The pids in the probe's log, in order.
fn pids(log: &std::path::Path) -> Vec<u64> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines = text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap());
    lines.filter_map(|l| l["pid"].as_u64()).collect()
}

#[tokio::test]
async fn one_service_serves_every_session_and_restarts_on_its_port() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("probe.jsonl");
    let driver = ScriptedDriver::new(4);
    driver.stall_sessions();
    let (path, text) = probe(&log, "[rigs.api]\npath = \"api\"");
    let files = [(path, text.as_str()), ("shop/api/.keep", "")];
    let w = world_driven(&files, driver);
    let (a, b) = (w.create().await, w.create_on("api").await);
    w.until_count("service.ready", 1).await;
    // Both sessions are running before the service is counted.
    w.events(&a, Duration::from_millis(500)).await;
    // A start may have failed before: another process took the port it picked.
    let failed = w.service_count("service.failed");
    assert_eq!(w.services(), 1, "one process for both sessions");
    let (sa, sb) = (
        w.driver.mcp(&a, 1)["probe"].clone(),
        w.driver.mcp(&b, 1)["probe"].clone(),
    );
    assert_eq!(sa["url"], sb["url"], "the same server");
    assert!(!sa["url"].as_str().unwrap().contains("AAE_SERVICE_PORT"), "{sa}");
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
    w.until_count("service.ready", 2).await;
    // More than one if another process held the port for a moment.
    assert!(w.service_count("service.failed") > failed);
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
    w.until_count("service.stopped", 1).await;
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
    let (path, text) = project("name = \"probe\"\ncommand = [\"false\"]\nmcp = true");
    let w = world_driven(&[(path, text.as_str())], ScriptedDriver::new(1));
    let run = w.create().await;
    w.until_count("service.failed", 1).await;
    // The launch waits for the service at most 30 s, then the session starts with a URL no one listens on.
    tokio::time::sleep(Duration::from_secs(31)).await;
    let states = w.states(&run).await;
    assert_eq!(states.last().unwrap(), "running", "{states:?}");
    let url = w.driver.mcp(&run, 1)["probe"]["url"].clone();
    assert!(url.as_str().unwrap().starts_with("http://127.0.0.1:0/"), "{url}");
}

#[tokio::test]
async fn a_daemon_crash_stops_the_service_it_left_and_the_next_session_gets_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("probe.jsonl");
    let driver = ScriptedDriver::new(1);
    driver.stall_sessions();
    let (path, text) = probe(&log, "");
    let mut w = world_driven(&[(path, text.as_str())], driver);
    let first = w.create().await;
    w.until_count("service.ready", 1).await;
    w.events(&first, Duration::from_millis(500)).await;
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
    w.until_count("service.ready", 2).await;
    w.events(&run, Duration::from_millis(500)).await;
    let server = w.driver.mcp(&run, 1)["probe"].clone();
    assert_eq!(call(&server), 200, "a new service for the new session");
    assert_ne!(*pids(&log).last().unwrap(), pid);
}

#[tokio::test]
async fn a_service_is_ready_only_once_its_health_check_passes_and_the_session_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let (log, flag) = (dir.path().join("probe.jsonl"), dir.path().join("up"));
    let service = format!(
        "name = \"probe\"\ncommand = [\"sh\", \"-c\", \"echo $AAE_SERVICE_PORT > \\\"$0/port\\\"; exec \\\"$1\\\" \\\"$2\\\"\", \
         \"{}\", \"{PROBE}\", \"{}\"]\nmcp = true\nhealth = {{ command = [\"test\", \"-e\", \"{}\"] }}",
        dir.path().display(),
        log.display(),
        flag.display()
    );
    let driver = ScriptedDriver::new(1);
    driver.stall_sessions();
    let (path, text) = project(&service);
    let w = world_driven(&[(path, text.as_str())], driver);
    let run = w.create().await;
    let port = file(&dir.path().join("port")).await;
    let bare = serde_json::json!({ "url": format!("http://127.0.0.1:{}/mcp", port.trim()), "headers": {} });
    // The port answers (the token is the session's, not known yet), the check does not pass yet.
    let start = Instant::now();
    while call(&bare) != 401 {
        assert!(start.elapsed() < Duration::from_secs(20), "the probe does not answer");
        std::thread::sleep(Duration::from_millis(20));
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(w.service_count("service.ready"), 0);
    assert!(
        w.driver.mcp(&run, 1).is_null(),
        "the session started before its service"
    );
    std::fs::write(&flag, "").unwrap();
    w.until_count("service.ready", 1).await;
    w.events(&run, Duration::from_millis(500)).await;
    assert_eq!(call(&w.driver.mcp(&run, 1)["probe"]), 200);
}

#[tokio::test(start_paused = true)]
async fn a_service_whose_health_check_never_passes_fails() {
    for health in ["health = { http = \"/nope\" }", "health = { command = [\"false\"] }"] {
        let dir = tempfile::tempdir().unwrap();
        let (path, text) = probe(&dir.path().join("probe.jsonl"), health);
        let w = world_driven(&[(path, text.as_str())], ScriptedDriver::new(1));
        w.create().await;
        w.until_count("service.starting", 1).await;
        tokio::time::sleep(Duration::from_secs(31)).await;
        assert_eq!(w.service_count("service.ready"), 0, "{health}");
        w.until_count("service.failed", 1).await;
    }
}

#[tokio::test]
async fn a_service_without_mcp_reaches_the_session_as_its_url_and_token() {
    let dir = tempfile::tempdir().unwrap();
    // `$0` holds the port only if aae put it there: sh does not expand its arguments.
    let service = format!(
        "name = \"mail-db\"\ncommand = [\"sh\", \"-c\", \"touch \\\"$0\\\"; exec sleep 60\", \"{}/port-${{AAE_SERVICE_PORT}}\"]\n\
         health = {{ command = [\"true\"] }}",
        dir.path().display()
    );
    let driver = ScriptedDriver::new(2);
    let (path, text) = project(&format!("{service}\n[rigs.api]\npath = \"api\""));
    let w = world_driven(&[(path, text.as_str()), ("shop/api/.keep", "")], driver);
    let (a, b) = (w.create().await, w.create_on("api").await);
    w.until_count("service.ready", 1).await;
    w.events(&b, Duration::from_millis(500)).await;
    assert_eq!(w.services(), 1, "one process for both sessions");
    let env = w.driver.sent(&a, 1).0.unwrap();
    let url = &env["AAE_SERVICE_MAIL_DB_URL"];
    let port = url.strip_prefix("http://127.0.0.1:").unwrap();
    // The check `true` may pass before the command ran.
    let start = Instant::now();
    while !dir.path().join(format!("port-{port}")).exists() {
        assert!(start.elapsed() < Duration::from_secs(20), "no port in its command");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!env["AAE_SERVICE_MAIL_DB_TOKEN"].is_empty());
    assert_eq!(w.driver.sent(&b, 1).0.unwrap()["AAE_SERVICE_MAIL_DB_URL"], *url);
    assert!(w.driver.mcp(&a, 1).get("mail-db").is_none(), "no MCP server");
    // It listens on no port: the launch waited for its check, not for a port.
    let states = w.states(&a).await;
    assert!(states.iter().any(|s| s == "running"), "{states:?}");
}

#[tokio::test]
async fn a_service_with_an_http_check_is_ready_once_it_answers() {
    let dir = tempfile::tempdir().unwrap();
    let (path, text) = probe(&dir.path().join("probe.jsonl"), "health = \"http\"");
    let w = world_driven(&[(path, text.as_str())], ScriptedDriver::new(1));
    w.create().await;
    w.until_count("service.ready", 1).await;
    // A start may fail before, on a port another process took: the check is what makes it ready.
    let failed = w.service_events().into_iter().filter(|(t, _)| t == "service.failed");
    for (_, data) in failed {
        assert!(data["failure"]["raw"].as_str().unwrap().contains("is taken"), "{data}");
    }
}

#[tokio::test]
async fn a_stdio_server_is_shared_behind_the_bridge_unless_it_is_per_session() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("probe.jsonl");
    let agent = format!(
        "harness = \"scripted\"\n[mcp.notes]\ncommand = [\"{PROBE}\", \"{}\"]\n[mcp.solo]\ncommand = [\"solo\"]\n\
         scope = \"session\"\n",
        log.display()
    );
    let driver = ScriptedDriver::new(2);
    driver.stall_sessions();
    let files = [
        ("shop/agents/dev/agent.toml", agent.as_str()),
        (
            "shop/aae.toml",
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        ),
        ("shop/api/.keep", ""),
    ];
    let w = world_driven(&files, driver);
    let (a, b) = (w.create().await, w.create_on("api").await);
    w.until_count("service.ready", 1).await;
    w.events(&b, Duration::from_millis(500)).await;
    assert_eq!(w.services(), 1, "one bridge for both sessions");
    let (na, nb) = (
        w.driver.mcp(&a, 1)["notes"].clone(),
        w.driver.mcp(&b, 1)["notes"].clone(),
    );
    assert_eq!(na["url"], nb["url"]);
    assert!(na["command"].is_null(), "{na}");
    assert_eq!(nb["headers"]["X-Aae-Run"], b.as_str());
    assert_eq!(call(&na), 200);
    assert_eq!(
        w.driver.mcp(&a, 1)["solo"]["command"][0],
        "solo",
        "per session: stdio as it is"
    );
}

/// Two projects with a `[[service]]` and a stdio server behind the bridge, both of `scope`: the services started and
/// whether the projects share each.
async fn two_projects(scope: &str) -> (usize, [bool; 2]) {
    let dir = tempfile::tempdir().unwrap();
    let (svc, notes) = (dir.path().join("svc.jsonl"), dir.path().join("notes.jsonl"));
    let config = format!(
        "projects = [\"shop\", \"mall\"]\nbuiltin = {{ default-policy = {{ enabled = false }} }}\n[harness.scripted]\n\
         [[service]]\nname = \"probe\"\ncommand = [\"{PROBE}\", \"{}\"]\nmcp = true\nscope = \"{scope}\"\n",
        svc.display()
    );
    // The same agent in both projects: one definition of the server.
    let agent = format!(
        "harness = \"scripted\"\n[mcp.notes]\ncommand = [\"{PROBE}\", \"{}\"]\nscope = \"{scope}\"\n",
        notes.display()
    );
    let build = "name = \"build\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n";
    let files = [
        ("config.toml", config.as_str()),
        ("mall/aae.toml", "[rigs.web]\npath = \"web\"\n"),
        ("mall/web/.keep", ""),
        ("shop/agents/dev/agent.toml", agent.as_str()),
        ("mall/agents/dev/agent.toml", agent.as_str()),
        ("mall/formulas/build.toml", build),
        ("mall/formulas/build.md", "Build."),
    ];
    let driver = ScriptedDriver::new(2);
    driver.stall_sessions();
    let w = world_driven(&files, driver);
    let (a, b) = (w.create().await, w.create_in("mall", "web").await);
    let processes = if scope == "project" { 4 } else { 2 };
    w.until_count("service.ready", processes).await;
    w.events(&b, Duration::from_millis(500)).await;
    let shared = ["probe", "notes"].map(|name| w.driver.mcp(&a, 1)[name]["url"] == w.driver.mcp(&b, 1)[name]["url"]);
    (w.services(), shared)
}

#[tokio::test]
async fn a_project_service_runs_once_per_project_an_orchestrator_one_once() {
    assert_eq!(two_projects("project").await, (4, [false, false]));
    assert_eq!(two_projects("orchestrator").await, (2, [true, true]));
}

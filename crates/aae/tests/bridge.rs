//! `aae mcp-bridge` (spec §11.4) as the daemon runs it: the real binary in front of the stdio server of
//! `aae_testkit::mcp`, which `cargo nextest run` builds as the `mcp-probe` binary next to `aae`.

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

const TOKEN: &str = "s3cret";
const MODERN: &str = "2026-07-28";

struct Bridge {
    child: Child,
    port: u16,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The bridge in front of `command`, or of the stdio probe logging to `<dir>/log`.
fn start(command: Option<&[&str]>) -> Bridge {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let probe = Path::new(env!("CARGO_BIN_EXE_aae")).with_file_name("mcp-probe");
    assert!(
        probe.exists(),
        "no {}: build it with `cargo build -p aae-testkit`",
        probe.display()
    );
    let own = [probe.to_str().unwrap(), "--stdio", log.to_str().unwrap()];
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let child = Command::new(env!("CARGO_BIN_EXE_aae"))
        .args(["mcp-bridge", "--"])
        .args(command.unwrap_or(&own))
        .env("AAE_SERVICE_PORT", port.to_string())
        .env("AAE_SERVICE_TOKEN", TOKEN)
        .spawn()
        .unwrap();
    Bridge {
        child,
        port,
        log,
        _dir: dir,
    }
}

/// Waits for `ready` for up to 10 s.
fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn accepts(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

impl Bridge {
    fn ready(self) -> Self {
        until("the port", || accepts(self.port));
        self
    }

    /// One HTTP request to `/mcp`: the status and the JSON body (`null` when empty).
    fn http(&self, method: &str, headers: &[(&str, &str)], body: &str) -> (u16, Value) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut request = format!("{method} /mcp HTTP/1.1\r\nhost: aae\r\nconnection: close\r\n");
        for (name, value) in headers {
            request += &format!("{name}: {value}\r\n");
        }
        request += &format!(
            "content-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        let status = reply[9..12].parse().unwrap();
        let body = reply.split_once("\r\n\r\n").unwrap().1;
        (status, serde_json::from_str(body).unwrap_or_default())
    }

    /// `POST /mcp` with the token, the run `run` and, if `modern`, the stateless epoch header.
    fn post(&self, body: &Value, run: &str, modern: bool) -> (u16, Value) {
        let bearer = format!("Bearer {TOKEN}");
        let mut headers = vec![
            ("authorization", bearer.as_str()),
            ("x-aae-run", run),
            ("x-aae-agent", "dev"),
        ];
        if modern {
            headers.push(("mcp-protocol-version", MODERN));
        }
        self.http("POST", &headers, &body.to_string())
    }

    fn log(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    fn exited(&mut self) -> bool {
        self.child.try_wait().unwrap().is_some()
    }
}

fn call(id: u64, sleep_ms: u64) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": "whoami_stdio", "arguments": { "sleep_ms": sleep_ms } } })
}

#[test]
fn the_port_opens_only_once_the_server_answered_initialize() {
    // `cat` echoes the handshake back as a request, never an answer: the server is never ready.
    let mut silent = start(Some(&["cat"]));
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        assert!(
            !accepts(silent.port),
            "the port opened before `initialize` was answered"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!silent.exited());

    let bridge = start(None).ready();
    let methods: Vec<Value> = bridge.log().iter().map(|line| line["method"].clone()).collect();
    assert_eq!(methods[..2], [json!("initialize"), json!("notifications/initialized")]);
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string();
    assert_eq!(bridge.http("POST", &[], &ping).0, 401);
    assert_eq!(bridge.http("POST", &[("authorization", "Bearer nope")], &ping).0, 401);
}

#[test]
fn both_epochs_are_answered_from_the_one_handshake() {
    let bridge = start(None).ready();
    let init = json!({ "jsonrpc": "2.0", "id": "a", "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t" } } });
    let (status, reply) = bridge.post(&init, "r1", false);
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["id"], "a");
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(reply["result"]["serverInfo"]["name"], "aae-mcp-stdio-probe");
    assert_eq!(reply["result"]["capabilities"], json!({ "tools": {} }));

    let discover = json!({ "jsonrpc": "2.0", "id": 2, "method": "server/discover" });
    let (_, reply) = bridge.post(&discover, "r1", true);
    assert_eq!(reply["result"]["supportedVersions"], json!([MODERN]), "{reply}");

    let (_, reply) = bridge.post(&call(3, 0), "r1", true);
    assert_eq!(reply["id"], 3);
    assert_eq!(reply["result"]["resultType"], "complete", "{reply}");
    let (_, reply) = bridge.post(&call(4, 0), "r1", false);
    assert_eq!(reply["result"].get("resultType"), None, "{reply}");
    let (_, reply) = bridge.post(&json!({ "jsonrpc": "2.0", "id": 5, "method": "nope" }), "r1", false);
    assert_eq!(reply["error"]["code"], -32601, "{reply}");

    let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(bridge.post(&notification, "r1", false).0, 202);
    assert_eq!(bridge.post(&json!([1]), "r1", false).0, 400);
    assert_eq!(
        bridge
            .http("GET", &[("authorization", &format!("Bearer {TOKEN}"))], "")
            .0,
        405
    );
    let initializes = bridge
        .log()
        .iter()
        .filter(|line| line["method"] == "initialize")
        .count();
    assert_eq!(initializes, 1, "a client's `initialize` reached the server");
}

#[test]
fn calls_with_one_id_from_two_clients_each_get_their_own_answer() {
    let bridge = start(None).ready();
    // The server answers one call at a time and each takes 300 ms, so both are in flight at the bridge at once.
    let (one, two) = std::thread::scope(|s| {
        let one = s.spawn(|| bridge.post(&call(1, 300), "run-1", false));
        let two = s.spawn(|| bridge.post(&call(1, 300), "run-2", true));
        (one.join().unwrap(), two.join().unwrap())
    });
    for ((status, reply), run) in [(one, "run-1"), (two, "run-2")] {
        assert_eq!(status, 200);
        assert_eq!(reply["id"], 1, "{reply}");
        let meta: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta["aae/run"], run, "{reply}");
        assert_eq!(meta["aae/agent"], "dev");
    }
    let log = bridge.log();
    assert!(
        log.iter().all(|line| line["pid"] == log[0]["pid"]),
        "more than one server"
    );
    assert_eq!(log.iter().filter(|line| line["method"] == "initialize").count(), 1);
}

#[test]
fn the_bridge_and_its_server_do_not_outlive_each_other() {
    let mut bridge = start(None).ready();
    let pid = bridge.log()[0]["pid"].to_string();
    assert!(Command::new("kill").args(["-9", &pid]).status().unwrap().success());
    until("the bridge to exit", || bridge.exited());
    assert!(!bridge.child.wait().unwrap().success());

    let mut bridge = start(None).ready();
    let pid = bridge.log()[0]["pid"].to_string();
    bridge.child.kill().unwrap();
    bridge.child.wait().unwrap();
    let alive = || {
        Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
    };
    until("the server to exit", || !alive().success());
}

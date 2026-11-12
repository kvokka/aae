//! Experiment 2 (spec §16): a codex app-server client over stdio or a WebSocket on a unix socket, and a fake
//! Responses API provider that answers every turn with a short text.

#[allow(clippy::all, clippy::pedantic)]
pub mod generated {
    pub mod protocol;
}

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How the client reaches app-server.
pub enum Wire {
    Stdio(ChildStdin),
    /// Outgoing frames go to the I/O thread that owns the WebSocket.
    Ws(Sender<String>),
}

pub struct Client {
    wire: Wire,
    rx: Receiver<Value>,
    next_id: u64,
    t0: Instant,
    /// Every message in both directions: (ms since t0, '>' or '<', message).
    pub log: Vec<(u128, char, Value)>,
}

/// Spawns `codex app-server` with the given transport (`stdio://` or `unix://PATH`) and environment.
pub fn spawn(listen: &str, env: &[(&str, String)]) -> Child {
    let mut cmd = Command::new("codex");
    cmd.args(["app-server", "--listen", listen]).env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    cmd.spawn().expect("spawn codex app-server")
}

impl Client {
    /// JSONL over the child's stdin/stdout.
    pub fn stdio(child: &mut Child) -> Client {
        let (tx, rx) = channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        Client::new(Wire::Stdio(child.stdin.take().unwrap()), rx)
    }

    /// WebSocket over a unix socket; one thread owns the socket and polls both directions.
    pub fn unix(path: &str) -> Client {
        let stream = (0..100)
            .find_map(|_| {
                UnixStream::connect(path).ok().or_else(|| {
                    std::thread::sleep(Duration::from_millis(50));
                    None
                })
            })
            .expect("connect to the app-server socket");
        let (mut ws, _) = tungstenite::client("ws://localhost/", stream.try_clone().unwrap()).expect("handshake");
        stream.set_read_timeout(Some(Duration::from_millis(10))).unwrap();
        let (tx_in, rx_in) = channel();
        let (tx_out, rx_out) = channel::<String>();
        std::thread::spawn(move || {
            loop {
                loop {
                    match rx_out.try_recv() {
                        Ok(text) => {
                            let _ = ws.send(tungstenite::Message::text(text));
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        // The client was dropped: close the connection like a restarting daemon would.
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            let _ = ws.close(None);
                            let _ = ws.flush();
                            return;
                        }
                    }
                }
                match ws.read() {
                    Ok(tungstenite::Message::Text(t)) => {
                        if let Ok(v) = serde_json::from_str(&t)
                            && tx_in.send(v).is_err()
                        {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                    Err(_) => return,
                }
            }
        });
        Client::new(Wire::Ws(tx_out), rx_in)
    }

    fn new(wire: Wire, rx: Receiver<Value>) -> Client {
        Client {
            wire,
            rx,
            next_id: 1,
            t0: Instant::now(),
            log: Vec::new(),
        }
    }

    pub fn ms(&self) -> u128 {
        self.t0.elapsed().as_millis()
    }

    pub fn send(&mut self, msg: Value) {
        let text = msg.to_string();
        self.log.push((self.ms(), '>', msg));
        match &mut self.wire {
            Wire::Stdio(w) => {
                let _ = writeln!(w, "{text}").and_then(|()| w.flush());
            }
            Wire::Ws(tx) => {
                let _ = tx.send(text);
            }
        }
    }

    /// Sends a request and returns its id without waiting.
    pub fn start(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"id": id, "method": method, "params": params}));
        id
    }

    /// Sends a request and waits for its response (`result` or `error`).
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.start(method, params);
        self.wait(Duration::from_secs(30), |m| m.get("method").is_none() && m["id"] == id)
            .unwrap_or_else(|| panic!("no response to {method}"))
    }

    /// Reads messages until `pred` matches; `None` on timeout or end of stream.
    pub fn wait(&mut self, timeout: Duration, pred: impl Fn(&Value) -> bool) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(m) => {
                    self.log.push((self.ms(), '<', m.clone()));
                    if pred(&m) {
                        return Some(m);
                    }
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    pub fn initialize(&mut self) -> Value {
        let r = self.request(
            "initialize",
            json!({"clientInfo": {"name": "aae-exp", "title": "aae-exp", "version": "0.0.1"},
                   "capabilities": {"experimentalApi": true}}),
        );
        self.send(json!({"method": "initialized"}));
        r
    }

    /// Messages received so far that carry `method`.
    pub fn seen(&self, method: &str) -> Vec<&Value> {
        self.log
            .iter()
            .filter(|(_, d, m)| *d == '<' && m["method"] == method)
            .map(|(_, _, m)| m)
            .collect()
    }
}

/// A request the fake provider received: the path, the `model` field and the whole body.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    /// Request line and headers.
    pub head: String,
    pub model: String,
    pub body: String,
}

/// Fake Responses API: replies to every `POST …/responses` with the text `OK` (or a command, see `RUN:`), after `SLOW<n>` seconds if the
/// last user text contains that word. Returns the base URL and the requests it got.
pub fn fake_provider() -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (n, conn) in listener.incoming().map_while(Result::ok).enumerate() {
            let log = log.clone();
            std::thread::spawn(move || serve(n, conn, &log));
        }
    });
    (base, seen)
}

fn serve(n: usize, conn: std::net::TcpStream, log: &Mutex<Vec<Seen>>) {
    let mut r = BufReader::new(conn.try_clone().unwrap());
    let (mut head, mut line) = (String::new(), String::new());
    while r.read_line(&mut line).unwrap_or(0) > 2 {
        head.push_str(&line);
        line.clear();
    }
    let lower = head.to_ascii_lowercase();
    let len = lower
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    let _ = r.read_exact(&mut body);
    let body = String::from_utf8_lossy(&body).into_owned();
    let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let model = v["model"].as_str().unwrap_or("").to_owned();
    log.lock().unwrap().push(Seen {
        path: path.clone(),
        head: head.clone(),
        model,
        body: body.clone(),
    });
    let mut w = conn;
    if !head.starts_with("POST") || !path.ends_with("/responses") {
        let _ = write!(
            w,
            "HTTP/1.1 404 Not Found\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
        );
        return;
    }
    let last_user = body.rsplit("input_text").next().unwrap_or("");
    if let Some(s) = last_user
        .split("SLOW")
        .nth(1)
        .and_then(|s| s.get(..1))
        .and_then(|d| d.parse().ok())
    {
        std::thread::sleep(Duration::from_secs(s));
    }
    // `RUN:<cmd>` in the last user message: answer with an `exec_command` call, then `OK` once its output is back.
    let user_at = body.rfind("\"role\":\"user\"").unwrap_or(0);
    let run = (body.rfind("function_call_output").is_none_or(|o| o < user_at))
        .then(|| body[user_at..].split("RUN:").nth(1)?.split('"').next())
        .flatten();
    let id = format!("resp_{n}");
    let events = [
        json!({"type": "response.created", "response": {"id": id}}),
        json!({"type": "response.output_item.done", "item": run.map_or_else(
            || json!({"type": "message", "role": "assistant", "id": format!("msg_{n}"),
                      "content": [{"type": "output_text", "text": "OK"}]}),
            |cmd| json!({"type": "function_call", "name": "exec_command", "call_id": format!("call_{n}"),
                         "arguments": json!({"cmd": cmd}).to_string()}))}),
        json!({"type": "response.completed", "response": {"id": id, "usage": {"input_tokens": 1,
               "output_tokens": 1, "total_tokens": 2}}}),
    ];
    let sse: String = events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
        .collect();
    let _ = write!(
        w,
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{sse}",
        sse.len()
    );
}

/// Resident set size of a process in kB.
pub fn rss_kb(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

pub struct Env {
    pub root: String,
    pub vars: Vec<(&'static str, String)>,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

/// A fresh root with an isolated `HOME`, a `CODEX_HOME` pointing at the fake provider and two work directories.
pub fn setup(name: &str) -> Env {
    let root = format!("{}/aae-exp2-{name}", std::env::temp_dir().display());
    let _ = std::fs::remove_dir_all(&root);
    for d in ["home", "codex-home", "ws-a", "ws-b", "sock"] {
        std::fs::create_dir_all(format!("{root}/{d}")).unwrap();
    }
    std::fs::set_permissions(format!("{root}/sock"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (base, seen) = fake_provider();
    let config = format!(
        "model = \"fake-default\"\nmodel_provider = \"fake\"\napproval_policy = \"never\"\n\
         sandbox_mode = \"danger-full-access\"\n[analytics]\nenabled = false\n[feedback]\nenabled = false\n\
         [model_providers.fake]\nname = \"fake\"\nbase_url = \"{base}\"\nwire_api = \"responses\"\n"
    );
    std::fs::write(format!("{root}/codex-home/config.toml"), config).unwrap();
    let vars = vec![
        ("PATH", std::env::var("PATH").unwrap()),
        ("HOME", format!("{root}/home")),
        ("CODEX_HOME", format!("{root}/codex-home")),
        ("SHELL", "/bin/sh".into()),
    ];
    Env { root, vars, seen }
}

pub fn thread_id(r: &Value) -> String {
    let resp: generated::protocol::ThreadStartResponse =
        serde_json::from_value(r["result"].clone()).expect("ThreadStartResponse");
    resp.thread.id
}

pub fn turn(c: &mut Client, thread: &str, text: &str, key: &str) -> u64 {
    c.start(
        "turn/start",
        json!({"threadId": thread, "clientUserMessageId": key,
               "input": [{"type": "text", "text": text, "text_elements": []}]}),
    )
}

/// Waits for `turn/completed` of a thread and returns its status.
pub fn completed(c: &mut Client, thread: &str, secs: u64) -> Option<generated::protocol::TurnStatus> {
    let m = c.wait(Duration::from_secs(secs), |m| {
        m["method"] == "turn/completed" && m["params"]["threadId"] == thread
    })?;
    let n: generated::protocol::TurnCompletedNotification =
        serde_json::from_value(m["params"].clone()).expect("TurnCompletedNotification");
    Some(n.turn.status)
}

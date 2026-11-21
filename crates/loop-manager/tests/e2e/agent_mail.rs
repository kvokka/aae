//! The addon `addons/agent-mail` of this repository on the real Agent Mail server `mcp-agent-mail` (pinned in
//! `mise.toml`), with no model: its launcher keeps the bearer contract of a service and the tool filter of the user's
//! config, a session keeps its mail name across a retry and a lost mailbox, and two runs of one agent in their own
//! worktrees write to each other, commit their own paths, and are refused a commit of a path the other holds.

use crate::common::{self, Daemon};
use loop_manager_testkit::anthropic::{Provider, Reply};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn addon() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../addons/agent-mail")
        .canonicalize()
        .unwrap()
}

fn helper() -> PathBuf {
    addon().join("bin/lm-agent-mail")
}

/// The HTTP status of a `POST` of `body` to `url`, with the bearer `token` when given, and the answer.
fn post(url: &str, token: Option<&str>, body: &Value) -> (String, String) {
    let mut curl = Command::new("curl");
    curl.args([
        "-sS",
        "--max-time",
        "20",
        "-w",
        "\n%{http_code}",
        "-H",
        "Content-Type: application/json",
    ])
    .args(["-H", "Accept: application/json, text/event-stream", "--data-binary"])
    .arg(body.to_string());
    if let Some(token) = token {
        curl.args(["-H", &format!("Authorization: Bearer {token}")]);
    }
    let out = curl.arg(url).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (answer, status) = text.rsplit_once('\n').unwrap_or_default();
    (status.to_owned(), answer.to_owned())
}

/// The names of the tools the server at `port` offers to a client with `token`.
fn tools(port: u16, token: &str) -> Vec<String> {
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    let (status, answer) = post(&format!("http://127.0.0.1:{port}/mcp/"), Some(token), &list);
    assert_eq!(status, "200", "{answer}");
    let answer: Value = serde_json::from_str(&answer).unwrap();
    let tools = answer["result"]["tools"].as_array().unwrap();
    tools.iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect()
}

/// A server the test started, killed when dropped.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The launcher as the daemon runs it: a clean environment with `home`, the port and the token.
fn serve(data: &Path, home: &Path, port: u16, token: &str) -> Server {
    let child = common::program(helper())
        .args(["serve"])
        .arg(data)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", home)
        .env("LOOP_MANAGER_SERVICE_PORT", port.to_string())
        .env("LOOP_MANAGER_SERVICE_TOKEN", token)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server(child);
    let ready = format!("http://127.0.0.1:{port}/health/readiness");
    let deadline = Instant::now() + common::WAIT;
    loop {
        let out = Command::new("curl")
            .args([
                "-sf",
                "-o",
                "/dev/null",
                "-H",
                &format!("Authorization: Bearer {token}"),
            ])
            .arg(&ready)
            .status()
            .unwrap();
        if out.success() {
            return server;
        }
        assert!(Instant::now() < deadline, "the server did not come up");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The helper's `command`, with `env` and `input` on stdin: its stdout, and stderr when it fails.
fn run(command: &[&str], env: &[(String, String)], input: &str) -> Result<String, String> {
    let mut child = common::program(helper())
        .args(command)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{text}{}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[test]
fn a_session_keeps_its_mail_name_across_a_retry_and_a_lost_mailbox() {
    // Agent Mail refuses a project under a directory named as `tempfile` names them.
    let tmp = tempfile::Builder::new().prefix("agent-mail-").tempdir().unwrap();
    let (data, home, notes) = (
        tmp.path().join("data"),
        tmp.path().join("home"),
        tmp.path().join("notes"),
    );
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&notes).unwrap();
    let (port, token) = (free_port(), "token-of-the-first-start");
    let server = serve(&data, &home, port, token);

    // A clean home: the compact tool set, and the daemon's bearer for every caller, loopback included.
    let mut offered = tools(port, token);
    offered.sort();
    assert_eq!(offered.len(), 12, "{offered:?}");
    assert!(offered.contains(&"check_file_reservation_conflicts".to_owned()));
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    let mcp = format!("http://127.0.0.1:{port}/mcp/");
    assert_eq!(post(&mcp, None, &list).0, "401");
    assert_eq!(post(&mcp, Some("another-token-entirely"), &list).0, "401");

    // A run outside Git: the mail project is the rig's path, the thread that of the run's chain.
    let event = json!({"project": "shop", "rig": "notes", "run": "run_7", "correlation": "cor_1",
        "data": {"workdir": notes, "run": {"agent": "kit.writer"}}});
    let decision = run(
        &["prepare"],
        &[(
            "LOOP_MANAGER_ADDON_PARAMS".into(),
            r#"{"project_key":"","ttl":900,"thread":""}"#.into(),
        )],
        &event.to_string(),
    )
    .unwrap();
    let decision: Value = serde_json::from_str(&decision).unwrap();
    let key = notes.canonicalize().unwrap();
    assert_eq!(
        decision,
        json!({"decision": "proceed", "env": {
            "LOOP_MANAGER_AGENT_MAIL_KEY": key, "LOOP_MANAGER_AGENT_MAIL_THREAD": "shop:cor_1",
            "LOOP_MANAGER_AGENT_MAIL_BINDING": "shop/notes/run_7/kit.writer", "LOOP_MANAGER_AGENT_MAIL_TTL": "900"}})
    );
    let mut env: Vec<(String, String)> = decision["env"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
        .collect();
    env.push((
        "LOOP_MANAGER_AGENT_MAIL_DATA".into(),
        data.to_string_lossy().into_owned(),
    ));
    let start = |env: &[(String, String)]| {
        let out: Value = serde_json::from_str(&run(&["session-start"], env, "{}").unwrap()).unwrap();
        let name = run(&["name"], env, "").unwrap_or_else(|e| panic!("{e}: {out}"));
        (
            name,
            out["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    };
    let (name, context) = start(&env);
    assert!(
        context.contains(&format!("you are `{name}`")) && context.contains("`shop:cor_1` as `thread_id`"),
        "{context}"
    );
    // A subagent works under its session's name: no registration of its own.
    let subagent = run(&["session-start"], &env, r#"{"agent_id":"a1"}"#).unwrap();
    assert_eq!(subagent, "");
    // A retry of the run's session gets the same name.
    assert_eq!(start(&env).0, name);

    // The mailbox lost, a new start on a new port and token: the session takes its name again and can reserve.
    drop(server);
    fs::remove_dir_all(data.join("mailbox")).unwrap();
    let (port, token) = (free_port(), "token-of-the-second-start");
    let _server = serve(&data, &home, port, token);
    assert_eq!(start(&env).0, name);
    let reserve = json!({"project_key": key, "agent_name": name, "paths": ["notes.md"], "exclusive": true});
    let granted: Value =
        serde_json::from_str(&run(&["call", "file_reservation_paths", &reserve.to_string()], &env, "").unwrap())
            .unwrap();
    assert_eq!(granted["granted"][0]["path_pattern"], "notes.md", "{granted}");
    // A tool the filter hides is named.
    let hidden = run(&["call", "ensure_project", r#"{"human_key":"/x"}"#], &env, "").unwrap_err();
    assert!(hidden.contains("tool `ensure_project` is not available"), "{hidden}");
}

/// What each session runs: the two runs meet, the first reserves `shared.txt` and writes to the second, the second
/// answers in the thread and is refused a commit of `shared.txt`; each commits a path it adds, one it changes and one
/// it deletes. The second declares a pass; the first stays until it is cancelled.
const SCENARIO: &str = r#"#!/bin/sh
set -eux
t=$1
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.com GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.com
# An exclusive create elects: two `mkdir` of one directory may both succeed on an overlay filesystem.
if (set -C && : >"$t/first") 2>/dev/null; then me=a peer=b; else me=b peer=a; fi
exec 2>"$t/$me.log"
name=$(lm-agent-mail name)
key=$LOOP_MANAGER_AGENT_MAIL_KEY
thread=$LOOP_MANAGER_AGENT_MAIL_THREAD
echo "$name" >"$t/$me.name"
echo "$LOOP_MANAGER_RUN" >"$t/$me.run"
wait_for() {
  n=0
  until [ -e "$1" ]; do n=$((n + 1)); [ "$n" -lt 900 ]; sleep 0.1; done
}
mail() {
  lm-agent-mail call "$1" "$(jq -nc --arg k "$key" --arg n "$name" --arg t "$thread" --arg p "$(cat "$t/$peer.name")" \
    "{project_key: \$k} + $2")"
}
own() {
  mail file_reservation_paths "{agent_name: \$n, paths: [\"$me-new.txt\", \"$me-changed.txt\", \"$me-gone.txt\"], exclusive: true}"
  echo "$me" >"$me-new.txt"
  echo "$me" >>"$me-changed.txt"
  git rm -q "$me-gone.txt"
  git add -A
  git commit -q -m "feat: own paths of $me"
}
wait_for "$t/$peer.name"
if [ "$me" = a ]; then
  mail file_reservation_paths '{agent_name: $n, paths: ["shared.txt"], exclusive: true}' >"$t/a.granted"
  mail send_message '{sender_name: $n, to: [$p], subject: "shared.txt", body_md: "I change shared.txt.", thread_id: $t,
    ack_required: true}'
  touch "$t/a.reserved"
  own
  echo a >>shared.txt
  git commit -q -a -m "feat: shared path of a"
  wait_for "$t/b.replied"
  mail fetch_inbox '{agent_name: $n, include_bodies: true}' >"$t/a.inbox"
  id=$(jq -r '.[0].id' "$t/a.inbox")
  mail acknowledge_message "{agent_name: \$n, message_id: $id}"
  touch "$t/a.done"
  wait_for "$t/never"
else
  wait_for "$t/a.reserved"
  mail fetch_inbox '{agent_name: $n, include_bodies: true}' >"$t/b.inbox"
  id=$(jq -r '.[0].id' "$t/b.inbox")
  mail reply_message "{sender_name: \$n, message_id: $id, body_md: \"Fine, shared.txt is yours.\"}"
  mail acknowledge_message "{agent_name: \$n, message_id: $id}"
  touch "$t/b.replied"
  head=$(git rev-parse HEAD)
  echo b >>shared.txt
  git add shared.txt
  if git commit -q -m "feat: shared path of b" 2>"$t/b.refusal"; then touch "$t/b.accepted"; fi
  [ "$(git rev-parse HEAD)" = "$head" ] && touch "$t/b.head-kept"
  git reset -q --hard
  if git commit -q --allow-empty -m "no conventional subject"; then touch "$t/b.subject-accepted"; fi
  own
  loop-manager finish pass --summary "b done"
fi
"#;

/// `git -C <dir> <args>` without the user's git config; its trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The user's own Agent Mail settings: a tool filter, anonymous loopback callers, and a mailbox of its own.
fn user_config(user_mailbox: &Path) -> String {
    format!(
        "TOOLS_FILTER_ENABLED=true\nTOOLS_FILTER_PROFILE=custom\nTOOLS_FILTER_MODE=exclude\n\
         TOOLS_FILTER_CLUSTERS=build_slots\nHTTP_ALLOW_LOCALHOST_UNAUTHENTICATED=true\n\
         STORAGE_ROOT={0}\nDATABASE_URL=sqlite:///{0}/storage.sqlite3\n",
        user_mailbox.display()
    )
}

/// The project `shop` in the daemon's tree under `dir`: the addon on every agent beside `conventional-commits`, the
/// agent `dev` on the fake provider at `base_url`, its worktree formula `build`, the rig `web` with its first commit,
/// and the user's `config.env`.
fn shop(dir: &Path, base_url: &str) {
    let shop = dir.join("config/loop-manager/shop");
    let files = [
        (
            dir.join("config/loop-manager/config.toml"),
            "projects = [\"shop\"]\n[harness.claude]\n".to_owned(),
        ),
        (
            shop.join("loop-manager.toml"),
            format!(
                "[[addon]]\npath = \"{}\"\n[[addon]]\npath = \"{}\"\n[settings]\naddons = [{{ name = \
                 \"conventional-commits\" }}, {{ name = \"agent-mail\", params = {{ thread = \"web-pair\", project_key = \
                 \"{KEY}\" }} }}]\n[rigs.web]\npath = \"web\"\nmax_writers = 2\n",
                addon().display(),
                addon().join("../conventional-commits").display()
            ),
        ),
        (
            shop.join("agents/dev/agent.toml"),
            format!(
                "harness = \"claude\"\nmodel = \"claude-haiku-5-5\"\ninherit_host = false\n\
                 [env]\nANTHROPIC_BASE_URL = \"{base_url}\"\nANTHROPIC_API_KEY = \"fake\"\n\
                 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n\
                 ENABLE_TOOL_SEARCH = \"false\"\n"
            ),
        ),
        (
            shop.join("formulas/build.toml"),
            "name = \"build\"\nworktree = true\n[[step]]\nkind = \"ai\"\nagent = \"dev\"\n".to_owned(),
        ),
        (
            shop.join("agents/dev/prompt.template.md"),
            "Build {{ rig.name }}.".to_owned(),
        ),
        (
            dir.join(".config/mcp-agent-mail/config.env"),
            user_config(&dir.join("user-mailbox")),
        ),
    ];
    for (path, text) in files {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let rig = shop.join("web");
    for file in [
        "shared.txt",
        "a-changed.txt",
        "a-gone.txt",
        "b-changed.txt",
        "b-gone.txt",
    ] {
        fs::write(rig.join(file), "base\n").unwrap();
    }
    git(&rig, &["init", "-q", "-b", "main"]);
    git(&rig, &["add", "-A"]);
    git(&rig, &["commit", "-q", "-m", "base"]);
}

/// The mail project of the rig `web`: a key of no temporary directory, which Agent Mail refuses.
const KEY: &str = "/srv/shop/web";

/// The paths of `paths` other agents than `me` hold, sorted, asked of the server whose data is `data`.
fn held(data: &Path, me: &str, paths: &[&str]) -> Vec<String> {
    let env = [(
        "LOOP_MANAGER_AGENT_MAIL_DATA".to_owned(),
        data.to_string_lossy().into_owned(),
    )];
    let args = json!({"project_key": KEY, "agent_name": me, "paths": paths});
    let out = run(
        &["call", "check_file_reservation_conflicts", &args.to_string()],
        &env,
        "",
    )
    .unwrap();
    let out: Value = serde_json::from_str(&out).unwrap();
    let conflicts = out["conflicts"].as_array().unwrap();
    let mut held: Vec<String> = conflicts
        .iter()
        .map(|c| c["path"].as_str().unwrap().to_owned())
        .collect();
    held.sort();
    held
}

/// Waits until `me` sees others hold exactly `want` of `paths`.
async fn until_held(data: &Path, me: &str, paths: &[&str], want: &[&str]) {
    let deadline = Instant::now() + common::WAIT;
    while held(data, me, paths) != want {
        assert!(Instant::now() < deadline, "{me} sees {:?}", held(data, me, paths));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// What a session left in `t`: its run, or its mail name (`what`), by its part `who`; empty before it is there.
fn left(t: &Path, who: &str, what: &str) -> String {
    fs::read_to_string(t.join(format!("{who}.{what}")))
        .map(|r| r.trim().to_owned())
        .unwrap_or_default()
}

/// Waits until the session `a` has its answer and the run `b` passed: the runs of `a` and `b`.
async fn until_b_passed(d: &Daemon, t: &Path) -> (String, String) {
    let log = |who: &str| fs::read_to_string(t.join(format!("{who}.log"))).unwrap_or_default();
    let deadline = Instant::now() + common::WAIT;
    loop {
        let b = left(t, "b", "run");
        if t.join("a.done").exists() && !b.is_empty() && d.get(&format!("/v1/runs/{b}")).await["state"] == "passed" {
            return (left(t, "a", "run"), b);
        }
        let runs = d.get("/v1/runs").await;
        assert!(Instant::now() < deadline, "a:\n{}\nb:\n{}\n{runs}", log("a"), log("b"));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The commits of each run on its branch: the paths it adds, changes and deletes, and `shared.txt` for `a` alone.
fn check_commits(rig: &Path, runs: [(&str, &str); 2]) {
    for (run, who) in runs {
        let changes = git(
            rig,
            &[
                "log",
                "--format=",
                "--name-status",
                &format!("main..loop-manager/{run}"),
            ],
        );
        for line in [
            format!("A\t{who}-new.txt"),
            format!("M\t{who}-changed.txt"),
            format!("D\t{who}-gone.txt"),
        ] {
            assert!(changes.lines().any(|l| l == line), "{run}: {changes}");
        }
        assert_eq!(
            changes.lines().any(|l| l == "M\tshared.txt"),
            who == "a",
            "{run}: {changes}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_runs_in_worktrees_coordinate_through_one_server() {
    let shared = tempfile::tempdir().unwrap();
    let t = shared.path().to_owned();
    fs::write(t.join("scenario.sh"), SCENARIO).unwrap();
    let scenario = format!("sh {0}/scenario.sh {0}", t.display());
    let provider = Provider::start(vec![Reply::Turn(vec![
        Reply::Bash(scenario),
        Reply::Text("Done.".into()),
    ])])
    .await;
    let base_url = provider.base_url();
    let d = Daemon::start(|dir| shop(dir, &base_url)).await;
    let dir = d.dir.path().to_owned();
    let (rig, data) = (
        dir.join("config/loop-manager/shop/web"),
        dir.join("state/loop-manager/addons/agent-mail"),
    );
    let config = dir.join(".config/mcp-agent-mail/config.env");
    let user_text = fs::read(&config).unwrap();

    let mut created = [d.create("build", None).await, d.create("build", None).await];
    let (a, b) = until_b_passed(&d, &t).await;
    let mut runs = [a.clone(), b.clone()];
    runs.sort();
    created.sort();
    assert_eq!(runs, created);
    let (name_a, name_b) = (left(&t, "a", "name"), left(&t, "b", "name"));
    assert_ne!(name_a, name_b);

    // One server for both sessions, which got its tools under the user's filter and their mail names as context.
    let instances = d.get("/v1/instances").await;
    let services = instances
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "service")
        .count();
    assert_eq!(services, 1, "{instances}");
    let endpoint: Value =
        serde_json::from_str(&fs::read_to_string(data.join("runtime/endpoint.json")).unwrap()).unwrap();
    let pid = endpoint["pid"].to_string();
    assert!(
        Command::new("kill").args(["-0", &pid]).status().unwrap().success(),
        "{endpoint}"
    );
    let bodies: Vec<String> = provider.requests().iter().map(|r| r.body.to_string()).collect();
    for who in [&name_a, &name_b] {
        assert!(
            bodies.iter().any(|b| b.contains(&format!("you are `{who}`"))),
            "no context for {who}"
        );
    }
    let tools = bodies
        .iter()
        .find(|b| b.contains("mcp__agent-mail__"))
        .expect("no tool of agent-mail");
    assert!(tools.contains("mcp__agent-mail__file_reservation_paths") && !tools.contains("acquire_build_slot"));

    // The managed server keeps the bearer contract and a mailbox of its own; the user's file is as it was.
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    assert_eq!(
        post(&format!("{}/mcp/", endpoint["url"].as_str().unwrap()), None, &list).0,
        "401"
    );
    assert_eq!(fs::read(&config).unwrap(), user_text);
    assert!(!dir.join("user-mailbox").exists());
    assert!(data.join("mailbox/storage.sqlite3").exists());

    // The message and its answer in one thread.
    for (who, from) in [("a", &name_b), ("b", &name_a)] {
        let inbox: Value = serde_json::from_str(&left(&t, who, "inbox")).unwrap();
        let got = (&inbox[0]["from"], &inbox[0]["thread_id"]);
        assert_eq!(got, (&json!(from), &json!("web-pair")), "{inbox}");
    }

    // A real commit of a path the other run holds is refused, naming it and its holder, and HEAD stays.
    let refusal = left(&t, "b", "refusal");
    assert!(
        refusal.contains(&format!("shared.txt: reserved by {name_a}")),
        "{refusal}"
    );
    assert!(!t.join("b.accepted").exists() && t.join("b.head-kept").exists());
    // The commit-msg hook of `conventional-commits` runs beside the check.
    assert!(!t.join("b.subject-accepted").exists());
    check_commits(&rig, [(&a, "a"), (&b, "b")]);
    // A commit outside the sessions goes as usual.
    fs::write(rig.join("shared.txt"), "by hand\n").unwrap();
    git(&rig, &["commit", "-q", "-a", "-m", "by hand"]);

    // The pass of b released its reservations only; the cancel of a releases a's.
    until_held(&data, &name_a, &["b-new.txt", "b-changed.txt"], &[]).await;
    assert_eq!(
        held(&data, &name_b, &["shared.txt", "a-new.txt"]),
        ["a-new.txt", "shared.txt"]
    );
    d.post(&format!("/v1/runs/{a}/cancel"), json!({}), None).await.unwrap();
    d.until_state(&a, "cancelled").await;
    until_held(&data, &name_b, &["shared.txt", "a-new.txt"], &[]).await;
}

//! Prompt runs through a real `aae daemon` and the real `claude` on the fake Anthropic API (spec §7.8, §15
//! criteria 2 and 9): the session calls the finisher with its Bash tool, the run passes. No model, no account.

mod common;

use aae_testkit::anthropic::{Provider, Reply};
use common::Daemon;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// A daemon whose `build` formula is a prompt run of a claude agent on the fake `provider`.
async fn daemon(provider: &Provider) -> Daemon {
    let base_url = provider.base_url();
    Daemon::start(|dir| {
        let shop = dir.join("config/aae/shop");
        let files = [
            (
                dir.join("config/aae/config.toml"),
                "projects = [\"shop\"]\n[harness.claude]\n".to_owned(),
            ),
            (
                shop.join("agents/dev/agent.toml"),
                format!(
                    "harness = \"claude\"\nmodel = \"claude-haiku-5-5\"\npermission_mode = \"bypassPermissions\"\n\
                     inherit_host = false\n[env]\nANTHROPIC_BASE_URL = \"{base_url}\"\nANTHROPIC_API_KEY = \"fake\"\n\
                     CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n"
                ),
            ),
            (
                shop.join("formulas/build.toml"),
                "name = \"build\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n".to_owned(),
            ),
            (shop.join("formulas/build.md"), "Build {{ rig.name }}.".to_owned()),
            (
                shop.join("formulas/fix.toml"),
                "name = \"fix\"\n[inputs]\nbead = { type = \"bead\", required = true }\n[step]\nkind = \"prompt\"\n\
                 agent = \"dev\"\nprompt = \"build.md\"\n"
                    .to_owned(),
            ),
        ];
        for (path, text) in files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        br(&shop, &["init", "--prefix", "t", "-q"]);
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_session_that_calls_the_finisher_passes_its_run() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let provider = Provider::start(vec![Reply::Bash(finish), Reply::Text("Done.".into())]).await;
    let d = daemon(&provider).await;
    let run = d.create("build", None).await;
    let view = d.until_state(&run, "passed").await;
    assert_eq!(view["step_kind"], "prompt");
    // Only the finisher passes a prompt run: the second model request carries the result of its Bash call.
    let requests = provider.requests();
    let result = requests[1].body["messages"].to_string();
    assert!(result.contains("tool_result"), "{result}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_live_view_of_a_run_relays_its_session_and_ends_with_the_run() {
    // The command sleeps first, so the view taps the session before the tool result comes back.
    let finish = format!("sleep 1 && {} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let provider = Provider::start(vec![Reply::Bash(finish), Reply::Text("Done.".into())]).await;
    let d = daemon(&provider).await;
    let run = d.create("build", None).await;
    let mut live = d.client.stream(&format!("/v1/runs/{run}/live")).await.unwrap();
    let (mut events, mut harness) = (Vec::new(), Vec::new());
    let watched = tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(event) = live.next().await.unwrap() {
            let data: Value = serde_json::from_str(&event.data).unwrap();
            match event.id {
                Some(_) => events.push(data),
                None => harness.push((event.event, data)),
            }
        }
    });
    watched.await.expect("the view ends with the run");
    assert!(events.iter().all(|e| e["run"] == run.as_str()), "{events:?}");
    assert_eq!(events[0]["type"], "run.queued");
    assert_eq!(events.last().unwrap()["type"], "run.passed");
    // The raw stream-json of the session: the tool result of the Bash call at least.
    assert!(harness.iter().all(|(kind, _)| kind == "harness"), "{harness:?}");
    assert!(harness.iter().any(|(_, m)| m["type"] == "user"), "{harness:?}");
    let attempt = &d.until_state(&run, "passed").await["attempts"][0];
    assert_eq!(attempt["harness"], "claude");
    assert!(attempt["session"]["session_id"].is_string(), "{attempt}");
}

/// br by hand in the project, as `alice`; its trimmed stdout.
fn br(project: &Path, args: &[&str]) -> String {
    let out = Command::new("br")
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

/// The status and assignee of a bead in br.
fn bead(project: &Path, id: &str) -> (Value, Value) {
    let shown: Value = serde_json::from_str(&br(project, &["show", id, "--json"])).unwrap();
    (shown[0]["status"].clone(), shown[0]["assignee"].clone())
}

/// The envelope of the next event of `kind` on the stream.
async fn next(events: &mut aae_front::client::Events, kind: &str) -> Value {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(20), events.next()).await;
        let event = event.expect("an event in time").unwrap().expect("the stream goes on");
        if event.event == kind {
            return serde_json::from_str(&event.data).unwrap();
        }
    }
}

/// The processes whose environment holds `AAE_RUN=<run>` and a path under `dir` (run ids repeat across
/// daemons): pid and command line.
fn of_run(run: &str, dir: &Path) -> Vec<(i32, String)> {
    let needle = format!("AAE_RUN={run}");
    let dir = dir.to_string_lossy();
    let entries = fs::read_dir("/proc").unwrap().flatten();
    entries
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            let env = fs::read(e.path().join("environ")).ok()?;
            let env = String::from_utf8_lossy(&env);
            (env.split('\0').any(|v| v == needle) && env.contains(&*dir)).then_some(())?;
            let cmd = fs::read(e.path().join("cmdline")).ok()?;
            Some((pid, String::from_utf8_lossy(&cmd).replace('\0', " ")))
        })
        .collect()
}

/// Criterion 9: `kill -9` of claude while its Bash tool runs is typed within a second, the session is resumed
/// and the run reaches its outcome. Criterion 16: the claim on the run's bead holds through the death of the session
/// and the resumed session closes it.
#[tokio::test(flavor = "multi_thread")]
async fn a_claude_killed_in_a_command_is_resumed_and_its_run_passes() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    // The resumed session sleeps a while first: time to look at the claim between the death and the outcome.
    let plan = vec![
        Reply::Bash("sleep 20".into()),
        Reply::Bash("sleep 3".into()),
        Reply::Bash(finish),
        Reply::Text("Done.".into()),
    ];
    let provider = Provider::start(plan).await;
    let d = daemon(&provider).await;
    let project = d.dir.path().join("config/aae/shop");
    let id = br(&project, &["create", "Fix the cart", "-l", "rig:web", "--silent"]);
    let mut events = d.client.events(Some(1)).await.unwrap();
    let body = json!({ "project": "shop", "rig": "web", "formula": "fix", "inputs": { "bead": id } });
    let run = d.post("/v1/runs", body, None).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let started = Instant::now();
    let claude = loop {
        let procs = of_run(&run, d.dir.path());
        let claude = procs.iter().find(|(_, cmd)| cmd.contains("--session-id"));
        if let Some((pid, _)) = claude.filter(|_| procs.iter().any(|(_, cmd)| cmd.contains("sleep 20"))) {
            break *pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no command running: {procs:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let killed = Instant::now();
    let status = std::process::Command::new("kill")
        .args(["-9", &claude.to_string()])
        .status();
    assert!(status.unwrap().success());
    let failure = next(&mut events, "run.attempt_failed").await["data"]["failure"].clone();
    let typed = killed.elapsed();
    assert_eq!(failure["kind"], "harness_crash", "{failure}");
    assert!(typed <= Duration::from_secs(1), "typed in {typed:?}");
    let owner = json!(format!("aae:{run}"));
    assert_eq!(
        bead(&project, &id),
        (json!("in_progress"), owner.clone()),
        "the claim holds"
    );
    d.until_state(&run, "passed").await;
    // The bead is closed by an effect after the run's own transaction.
    next(&mut events, "bead.closed").await;
    assert_eq!(bead(&project, &id), (json!("closed"), owner));
    assert!(provider.requests().len() >= 3, "the resumed session went on");
}

/// Criterion 4a on the fake: the finisher is called while a background command of the session runs; the run
/// passes only once the session is idle without background, so never before the command is done.
#[tokio::test(flavor = "multi_thread")]
async fn a_finisher_called_with_a_background_command_running_waits_for_it() {
    let finish = format!("{} finish pass --summary built", env!("CARGO_BIN_EXE_aae"));
    let plan = vec![
        Reply::Background("sleep 3 && touch background-done".into()),
        Reply::Bash(finish),
        Reply::Text("Done.".into()),
    ];
    let provider = Provider::start(plan).await;
    let d = daemon(&provider).await;
    let run = d.create("build", None).await;
    d.until_state(&run, "passed").await;
    let rig = d.dir.path().join("config/aae/shop/web");
    assert!(
        rig.join("background-done").exists(),
        "passed before the background command ended"
    );
}

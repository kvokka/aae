//! The addon `addons/lessons` of this repository on a real daemon, with the real `claude` on the fake Anthropic API and
//! the real `codex app-server` on the fake Responses API: once the agent declared its outcome, the same session is asked
//! for its lessons, and a useful answer goes on the run's bead once, an empty one nowhere (addons/lessons/README.md).

use crate::common::{self, Daemon};
use loop_manager_testkit::{anthropic, openai};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Stdio};

fn addon() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addons/lessons")
}

/// What the agent of the tests is asked after its outcome: a phrase of the addon's request.
const ASKED: &str = "ASD-STE100 Simplified Technical English";
const NOTE: &str = "- When `cargo test` cannot find the fixture, run it from the crate directory.";

/// A daemon whose formula `fix` is a run of the agent `dev` on a bead, `agent` its harness settings, with the addon
/// attached and `loop-manager` on the `PATH` of its sessions.
async fn daemon(agent: &str) -> Daemon {
    daemon_with("[{ name = \"lessons\" }]", agent).await
}

/// [`daemon`] with `addons` attached to `dev`; the addon `context-budget` of this repository is declared too.
async fn daemon_with(addons: &str, agent: &str) -> Daemon {
    let budget = addon().join("../context-budget");
    Daemon::start(|dir| {
        let shop = dir.join("config/loop-manager/shop");
        let files = [
            (
                dir.join("config/loop-manager/config.toml"),
                format!(
                    "projects = [\"shop\"]\n[harness.claude]\n[harness.codex]\n[[addon]]\npath = \"{}\"\n\
                     [[addon]]\npath = \"{}\"\n",
                    addon().display(),
                    budget.display()
                ),
            ),
            (
                shop.join("agents/dev/agent.toml"),
                format!("addons = {addons}\n{agent}"),
            ),
            (
                shop.join("agents/dev/prompt.template.md"),
                "Fix {{ inputs.bead }}.".to_owned(),
            ),
            (
                shop.join("formulas/fix.toml"),
                "name = \"fix\"\n[inputs]\nbead = { type = \"task\", required = true }\n[[step]]\nkind = \"ai\"\n\
                 agent = \"dev\"\n"
                    .to_owned(),
            ),
        ];
        for (path, text) in files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fs::create_dir_all(dir.join("path")).unwrap();
        std::os::unix::fs::symlink(common::EXE, dir.join("path/loop-manager")).unwrap();
        br(&shop, &["init", "--prefix", "t", "-q"]);
    })
    .await
}

fn claude(provider: &anthropic::Provider) -> String {
    format!(
        "harness = \"claude\"\nmodel = \"claude-haiku-5-5\"\ninherit_host = false\n\
         [env]\nANTHROPIC_BASE_URL = \"{}\"\nANTHROPIC_API_KEY = \"fake\"\n\
         CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n",
        provider.base_url()
    )
}

fn codex(provider: &openai::Provider) -> String {
    // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
    format!(
        "harness = \"codex\"\nmodel = \"gpt-fake\"\ninherit_host = false\n\
         [native.codex]\nmodel_provider = \"fake\"\nsandbox_mode = \"danger-full-access\"\n\
         [native.codex.model_providers.fake]\nname = \"fake\"\nbase_url = \"{}\"\nwire_api = \"responses\"\n",
        provider.base_url()
    )
}

/// br by hand in the project; its trimmed stdout.
fn br(project: &Path, args: &[&str]) -> String {
    let out = common::program("br")
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

/// The texts of the comments on a bead, oldest first.
fn comments(project: &Path, id: &str) -> Vec<String> {
    let listed: Value = serde_json::from_str(&br(project, &["comments", "list", id, "--json"])).unwrap();
    let listed = listed.as_array().cloned().unwrap_or_default();
    listed
        .iter()
        .map(|c| c["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// The run of `fix` on a new bead, beside another bead, until it passes: the project, the two beads and the run.
async fn fix(d: &Daemon) -> (std::path::PathBuf, String, String, String) {
    let project = d.dir.path().join("config/loop-manager/shop");
    let other = br(&project, &["create", "Tidy the docs", "--silent"]);
    let id = br(&project, &["create", "Fix the cart", "--silent"]);
    let body = json!({ "project": "shop", "rig": "web", "formula": "fix", "inputs": { "bead": id } });
    let run = d.post("/v1/runs", body, None).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let view = d.until_state(&run, "passed").await;
    // The request for lessons leaves the declared outcome as it was.
    assert_eq!(view["outcome"]["summary"], "fixed", "{view}");
    (project, id, other, run)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_agent_is_asked_in_its_session_and_its_lessons_reach_its_bead_once() {
    use anthropic::Reply;
    let finish = format!("{} finish pass --summary fixed", common::EXE);
    let plan = vec![
        Reply::Bash(finish),
        Reply::Text("Done.".into()),
        Reply::Text(NOTE.into()),
    ];
    let provider = anthropic::Provider::start(plan).await;
    let d = daemon(&claude(&provider)).await;
    let (project, id, other, run) = fix(&d).await;
    // One request after the finished turn, and none after the answer to it.
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "{:?}",
        requests.iter().map(|r| &r.body).collect::<Vec<_>>()
    );
    // The request goes on the conversation of the finished turn: its prompt, the finisher's result, its last answer.
    let messages = requests[2].body["messages"].to_string();
    for part in [format!("Fix {id}."), "tool_result".into(), "Done.".into(), ASKED.into()] {
        assert!(messages.contains(&part), "{part}: {messages}");
    }
    assert_eq!(comments(&project, &id), [format!("LESSONS: {run}\n\n{NOTE}")]);
    assert!(comments(&project, &other).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_agent_with_no_lesson_leaves_no_comment() {
    use anthropic::Reply;
    let finish = format!("{} finish pass --summary fixed", common::EXE);
    let plan = vec![
        Reply::Bash(finish),
        Reply::Text("Done.".into()),
        Reply::Text("NONE".into()),
    ];
    let provider = anthropic::Provider::start(plan).await;
    let d = daemon(&claude(&provider)).await;
    let (project, id, _, _) = fix(&d).await;
    assert_eq!(provider.requests().len(), 3);
    assert!(comments(&project, &id).is_empty());
}

/// `addons/context-budget` beside the addon, with a hard limit any session passes on its first measure.
const STOPPED: &str = "[{ name = \"lessons\" }, { name = \"context-budget\", params = { soft = 1, hard = 5 } }]";

/// The run of `fix` whose session `context-budget` stopped: no request for lessons reached the model, and the bead holds
/// the dump of the stop alone. `requests` gives the texts of the requests to the model.
async fn stopped_and_not_asked(d: &Daemon, requests: impl Fn() -> Vec<String>) {
    let project = d.dir.path().join("config/loop-manager/shop");
    let id = br(&project, &["create", "Fix the cart", "--silent"]);
    let body = json!({ "project": "shop", "rig": "web", "formula": "fix", "inputs": { "bead": id } });
    let run = d.post("/v1/runs", body, None).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let view = d.until_state(&run, "failed").await;
    let cut = "context budget: the session was stopped; its state is on the bead";
    assert_eq!(view["outcome"]["summary"], cut, "{view}");
    let requests = requests();
    assert!(requests.iter().all(|r| !r.contains(ASKED)), "{requests:?}");
    let comments = comments(&project, &id);
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(
        comments[0].starts_with("# context-budget: the session was stopped"),
        "{comments:?}"
    );
    let data = d.dir.path().join("state/loop-manager/addons/lessons");
    assert_eq!(fs::read_dir(data).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_session_stopped_by_its_context_budget_is_not_asked() {
    use anthropic::Reply;
    // claude writes the transcript the budget is measured on after the first call.
    let plan = vec![
        Reply::Bash("true".into()),
        Reply::Bash("true".into()),
        Reply::Text("Done.".into()),
        Reply::Text(NOTE.into()),
    ];
    let provider = anthropic::Provider::start(plan).await;
    let d = daemon_with(STOPPED, &claude(&provider)).await;
    let texts = || provider.requests().iter().map(|r| r.body.to_string()).collect();
    stopped_and_not_asked(&d, texts).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_codex_session_stopped_by_its_context_budget_is_not_asked() {
    use openai::Reply;
    let plan = vec![
        Reply::Command("true".into()),
        Reply::Command("true".into()),
        Reply::Text("Done.".into()),
        Reply::Text(NOTE.into()),
    ];
    let provider = openai::Provider::start(plan).await;
    let d = daemon_with(STOPPED, &codex(&provider)).await;
    let texts = || provider.requests().iter().map(|r| r.body.to_string()).collect();
    stopped_and_not_asked(&d, texts).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_codex_agent_is_asked_in_its_thread_and_its_lessons_reach_its_bead_once() {
    use openai::Reply;
    let finish = format!("{} finish pass --summary fixed", common::EXE);
    let plan = vec![
        Reply::Command(finish),
        Reply::Text("Done.".into()),
        Reply::Text(NOTE.into()),
    ];
    let provider = openai::Provider::start(plan).await;
    let d = daemon(&codex(&provider)).await;
    let (project, id, other, run) = fix(&d).await;
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "{:?}",
        requests.iter().map(|r| &r.body).collect::<Vec<_>>()
    );
    let input = requests[2].body["input"].to_string();
    for part in [
        format!("Fix {id}."),
        "function_call_output".into(),
        "Done.".into(),
        ASKED.into(),
    ] {
        assert!(input.contains(&part), "{part}: {input}");
    }
    assert_eq!(comments(&project, &id), [format!("LESSONS: {run}\n\n{NOTE}")]);
    assert!(comments(&project, &other).is_empty());
}

/// The hook by hand, with `stand-ins` first on its `PATH`, in a session of run `r1` on bead `t-1`: its stdout.
fn hook(data: &Path, stand_ins: &Path, input: &Value) -> String {
    hook_on(Some("t-1"), data, stand_ins, input)
}

/// [`hook`] in a session of run `r1` on `bead`, or on no bead.
fn hook_on(bead: Option<&str>, data: &Path, stand_ins: &Path, input: &Value) -> String {
    use std::io::Write;
    let path = std::env::join_paths(
        std::iter::once(stand_ins.to_owned()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = common::program(addon().join("hooks/lessons"));
    command.arg(data).env("PATH", path).env("LOOP_MANAGER_RUN", "r1");
    match bead {
        Some(bead) => command.env("LOOP_MANAGER_BEAD", bead),
        None => command.env_remove("LOOP_MANAGER_BEAD"),
    };
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn the_hook_asks_once_per_session_and_publishes_only_the_answer_to_its_request() {
    let tmp = tempfile::tempdir().unwrap();
    let (data, stand_ins) = (tmp.path().join("data"), tmp.path().join("bin"));
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&stand_ins).unwrap();
    let outcome = tmp.path().join("outcome.json");
    let posted = tmp.path().join("posted");
    let stand_in = |name: &str, body: String| {
        let file = stand_ins.join(name);
        fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    };
    stand_in("loop-manager", format!("cat '{}'", outcome.display()));
    // The comment's text, with the bead it went on.
    stand_in(
        "br",
        format!("echo \"$3\" >>'{0}'; cat \"$5\" >>'{0}'", posted.display()),
    );
    let stop = |session: &str, active: bool, message: &str| {
        json!({ "hook_event_name": "Stop", "session_id": session, "stop_hook_active": active,
                "last_assistant_message": message })
    };
    let posted_text = || fs::read_to_string(&posted).unwrap_or_default();

    // No outcome declared yet: the turn ends as it would.
    fs::write(&outcome, r#"{"state":"running","outcome":null}"#).unwrap();
    assert_eq!(hook(&data, &stand_ins, &stop("s1", false, "Working.")), "");
    // A subagent's stop asks nothing, outcome or not.
    fs::write(
        &outcome,
        r#"{"state":"running","outcome":{"result":"pass","summary":"fixed"}}"#,
    )
    .unwrap();
    let mut sub = stop("s1", false, "Found it.");
    sub["agent_id"] = json!("a1");
    assert_eq!(hook(&data, &stand_ins, &sub), "");
    // The outcome is declared: the session is asked.
    let asked: Value = serde_json::from_str(&hook(&data, &stand_ins, &stop("s1", false, "Done."))).unwrap();
    assert_eq!(asked["decision"], "block");
    assert!(asked["reason"].as_str().unwrap().contains(ASKED), "{asked}");
    // Its answer goes on the bead once.
    assert_eq!(hook(&data, &stand_ins, &stop("s1", true, NOTE)), "");
    assert_eq!(posted_text(), format!("t-1\nLESSONS: r1\n\n{NOTE}"));
    // A later stop of the session, after a remark or another hook's block, neither asks nor publishes.
    assert_eq!(hook(&data, &stand_ins, &stop("s1", true, NOTE)), "");
    assert_eq!(hook(&data, &stand_ins, &stop("s1", false, "Fixed again.")), "");
    assert_eq!(posted_text(), format!("t-1\nLESSONS: r1\n\n{NOTE}"));
    // Another session is asked on its own; its empty answer, in any spelling of NONE, publishes nothing.
    for (session, answer) in [("s2", "NONE"), ("s3", "  none.\n"), ("s4", "")] {
        let asked = hook(&data, &stand_ins, &stop(session, false, "Done."));
        assert!(asked.contains("\"block\""), "{asked}");
        assert_eq!(hook(&data, &stand_ins, &stop(session, true, answer)), "");
    }
    // A session asked whose answer never ended its turn, as one resumed after its attempt failed: its next stop is not
    // an answer, and its last message is the work's, not a lesson.
    assert!(hook(&data, &stand_ins, &stop("s5", false, "Done.")).contains("\"block\""));
    assert_eq!(hook(&data, &stand_ins, &stop("s5", false, "Fixed again.")), "");
    assert_eq!(posted_text(), format!("t-1\nLESSONS: r1\n\n{NOTE}"));
    // A run on no bead is not asked, outcome or not.
    assert_eq!(hook_on(None, &data, &stand_ins, &stop("s6", false, "Done.")), "");
    assert!(!data.join("s6.asked").exists());
}

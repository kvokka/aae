//! Demo 3 (`demos/3-review-gate`) without a model and without GitHub: its pack, its project, the scripts of its steps
//! and its bridge against a real `loop-manager daemon`, the real `codex` on fake Responses APIs, and a fake `gh`
//! (`fake_gh.sh`) over local remotes. The planner files one bead for the backend, whose coder adds a file, so a
//! feature has a pull request in the backend and one in the root.

use crate::common::{self, Daemon, WAIT};
use loop_manager_testkit::openai::{Provider, Reply};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

const PROJECT: &str = "review-gate";
const REPOS: [&str; 3] = ["gate-demo", "gate-demo-backend", "gate-demo-frontend"];
/// The author the demo's `feedback_from` names.
const ALLOWED: &str = "kvokka-agent";

/// The daemon on a copy of the demo, with the bridge running; `tools` holds the agents' scripts.
struct Gate {
    d: Daemon,
    bridge: Child,
    tools: tempfile::TempDir,
    _fakes: Vec<Provider>,
}

impl Drop for Gate {
    fn drop(&mut self) {
        // TERM, so the bridge stops its follower of the events too.
        let _ = Command::new("kill")
            .args(["-TERM", &self.bridge.id().to_string()])
            .status();
        let _ = self.bridge.wait();
    }
}

fn demo_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/3-review-gate")
}

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    run(Command::new("git").arg("-C").arg(dir).args(args))
}

/// The demo's pack and project under `dir/demo`, each agent that runs on its fake, the repositories as clones of bare
/// remotes in `dir/remotes`, and the fake `gh` with `loop-manager` in `dir/path`.
fn lay_out(dir: &Path, fakes: &[(&str, &Provider)]) {
    let (src, demo) = (demo_dir(), dir.join("demo"));
    fs::create_dir_all(demo.join("root")).unwrap();
    for (from, to) in [("pack", "."), ("root/agents", "root"), ("root/formulas", "root")] {
        run(Command::new("cp").arg("-R").arg(src.join(from)).arg(demo.join(to)));
    }
    fs::copy(src.join("root/loop-manager.toml"), demo.join("root/loop-manager.toml")).unwrap();
    for (agent, fake) in fakes {
        let path = demo.join(agent).join("agent.toml");
        // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
        let native = format!(
            "\n[native.codex]\nmodel_provider = \"fake\"\nsandbox_mode = \"danger-full-access\"\n\
             [native.codex.model_providers.fake]\nname = \"fake\"\nbase_url = \"{}\"\n\
             wire_api = \"responses\"\nrequest_max_retries = 0\nstream_max_retries = 0\n",
            fake.base_url()
        );
        fs::write(&path, fs::read_to_string(&path).unwrap() + &native).unwrap();
    }
    let root = demo.join("root");
    // The demo's `[exec]`, the last table of its orchestrator config, with the daemon's GITHUB_TOKEN by reference.
    let demo_config = fs::read_to_string(src.join("root/config.toml")).unwrap();
    let exec = &demo_config[demo_config.find("\n[exec]").unwrap()..];
    let config = format!("projects = [\"{}\"]\n{exec}", root.display());
    fs::write(dir.join("config/loop-manager/config.toml"), config).unwrap();
    let daemon = dir.join("bin/loop-manager");
    fs::create_dir_all(dir.join("bin")).unwrap();
    let wrapper = format!("#!/bin/sh\nGITHUB_TOKEN=gate-token exec {} \"$@\"\n", common::EXE);
    fs::write(&daemon, wrapper).unwrap();
    run(Command::new("chmod").arg("+x").arg(&daemon));
    run(common::program("br").args(["init", "-q"]).current_dir(&root));
    for repo in REPOS {
        let (remote, clone) = (dir.join("remotes").join(format!("{repo}.git")), demo.join(repo));
        run(Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&remote));
        run(Command::new("git").args(["init", "-q", "-b", "main"]).arg(&clone));
        git(&clone, &["config", "user.name", "Gate"]);
        git(&clone, &["config", "user.email", "gate@example.com"]);
        git(&clone, &["commit", "-q", "--allow-empty", "-m", "init"]);
        git(&clone, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&clone, &["push", "-q", "origin", "main"]);
        git(&clone, &["remote", "set-head", "origin", "-a"]);
    }
    let tools = dir.join("path");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("gh"), include_str!("fake_gh.sh")).unwrap();
    run(Command::new("chmod").arg("+x").arg(tools.join("gh")));
    std::os::unix::fs::symlink(common::EXE, tools.join("loop-manager")).unwrap();
}

/// The PATH of the daemon (`common::command`): the test's tools first.
fn path(dir: &Path) -> std::ffi::OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.join("path")).chain(std::env::split_paths(&path))).unwrap()
}

impl Gate {
    async fn start() -> Self {
        let loop_manager = common::EXE;
        let tools = tempfile::tempdir().unwrap();
        // The planner files a bead for the backend, and refuses the feature once `refuse` exists.
        let refuse = tools.path().join("refuse");
        let plan = format!(
            "set -e\nif [ -e {} ]; then {loop_manager} finish fail --summary refused; exit; fi\n\
             bead=$({loop_manager} --json run show \"$LOOP_MANAGER_RUN\" | jq -r .bead)\n\
             BD_ISSUE_PREFIX=api br create Greet --deps parent-child:$bead -l agent:coder --description Greet --silent\n\
             {loop_manager} finish pass --summary planned\n",
            refuse.display()
        );
        fs::write(tools.path().join("plan.sh"), plan).unwrap();
        let task = |command: String| Provider::start(vec![Reply::Task(command)]);
        let planner = task(format!("sh {}", tools.path().join("plan.sh").display())).await;
        let coder = task(format!(
            "echo hello >greet.txt && {loop_manager} finish pass --summary coded"
        ))
        .await;
        let reviser = task(format!("{loop_manager} finish pass --summary addressed")).await;
        let owner = task(format!("{loop_manager} finish pass --summary looked")).await;
        let fakes = [
            ("pack/agents/planner", &planner),
            ("pack/agents/coder", &coder),
            ("pack/agents/reviser", &reviser),
            ("root/agents/owner", &owner),
        ];
        let d = Daemon::start(|dir| lay_out(dir, &fakes)).await;
        let bridge = Command::new("sh")
            .arg(demo_dir().join("bridge.sh"))
            .arg(PROJECT)
            .env("LOOP_MANAGER", loop_manager)
            .env("INTERVAL", "1")
            .env("LOOP_MANAGER_SOCKET", d.socket())
            .env("PATH", path(d.dir.path()))
            .env("HOME", d.dir.path())
            .env("XDG_CONFIG_HOME", d.dir.path().join("config"))
            .spawn()
            .unwrap();
        Self {
            d,
            bridge,
            tools,
            _fakes: vec![planner, coder, reviser, owner],
        }
    }

    fn demo(&self, path: &str) -> PathBuf {
        self.d.dir.path().join("demo").join(path)
    }

    /// A run of `dev.feature` with `inputs`.
    async fn feature(&self, inputs: Value) -> String {
        let body = json!({ "project": PROJECT, "formula": "dev.feature", "inputs": inputs });
        let reply = self.d.post("/v1/runs", body, None).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// Waits until `run` waits on `review` in `round`.
    async fn waiting(&self, run: &str, round: u64) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let view = self.d.get(&format!("/v1/runs/{run}")).await;
            if view["state"] == "running" && view["step"] == "review" && view["round"] == round {
                return view;
            }
            assert!(
                Instant::now() < deadline,
                "{run} does not wait in round {round}: {view}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The bridge's looks at the root's pull request so far: each one starts with it.
    fn looks(&self) -> usize {
        let calls = fs::read_to_string(self.d.dir.path().join("path/github/calls")).unwrap();
        calls.lines().filter(|c| c.ends_with("--json state,mergedBy")).count()
    }

    /// The bridge looks at GitHub once more from start to end, and `run` still waits in `round`.
    async fn still(&self, run: &str, round: u64) {
        // The second look from now starts after the first one ended, and the first started after the call.
        let (now, deadline) = (self.looks(), Instant::now() + WAIT);
        while self.looks() < now + 2 {
            assert!(Instant::now() < deadline, "the bridge does not look");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let view = self.d.get(&format!("/v1/runs/{run}")).await;
        assert_eq!(
            (&view["step"], &view["round"]),
            (&json!("review"), &json!(round)),
            "{view}"
        );
        assert_eq!(view["state"], "running", "{view}");
    }

    /// The fake `gh` as `user`, in the root's repository.
    fn gh(&self, user: &str, args: &[&str]) -> String {
        self.gh_in("gate-demo", user, args)
    }

    /// The fake `gh` as `user`, in the repository `repo`.
    fn gh_in(&self, repo: &str, user: &str, args: &[&str]) -> String {
        let gh = self.d.dir.path().join("path/gh");
        run(Command::new(gh)
            .args(args)
            .env("GH_FAKE_USER", user)
            .current_dir(self.demo(repo)))
    }

    /// The backend's pull request of the feature `view`: its address and its state.
    fn backend_pr(&self, view: &Value) -> (String, String) {
        let branch = format!("feature/{}", view["bead"].as_str().unwrap());
        let pr = self.gh_in("gate-demo-backend", "", &["pr", "view", &branch, "--json", "url,state"]);
        let pr: Value = serde_json::from_str(&pr).unwrap();
        (
            pr["url"].as_str().unwrap().to_owned(),
            pr["state"].as_str().unwrap().to_owned(),
        )
    }

    async fn runs_of(&self, formula: &str) -> Vec<Value> {
        let runs = self.d.get(&format!("/v1/runs?project={PROJECT}")).await;
        let runs = runs.as_array().unwrap().iter();
        runs.filter(|r| r["formula"] == formula).cloned().collect()
    }
}

/// The root's pull request: the last line of the output of `pr`.
fn root_pr(view: &Value) -> String {
    let out = view["steps"]["pr"]["data"]["stdout"].as_str().unwrap();
    out.lines().last().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_of_the_root_pull_request_passes_the_review() {
    let g = Gate::start().await;
    let run = g.feature(json!({ "task": "Greet the visitor" })).await;
    let pr = root_pr(&g.waiting(&run, 1).await);
    g.gh(ALLOWED, &["pr", "merge", &pr, "--merge"]);
    let view = g.d.until_state(&run, "passed").await;
    assert_eq!(
        view["steps"]["review"]["data"],
        json!({ "merged_by": ALLOWED }),
        "{view}"
    );
    assert_eq!(view["step"], "merge", "{view}");
    let merged = view["outcome"]["data"]["stdout"].as_str().unwrap();
    assert!(merged.contains("MY_AWESOME_ENV_VAR is something"), "{merged}");
    assert_eq!(g.backend_pr(&view).1, "MERGED", "{view}");
    // `merge` took each repository back to its default branch.
    for repo in REPOS {
        assert_eq!(git(&g.demo(repo), &["branch", "--show-current"]), "main\n", "{repo}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_comment_of_an_allowed_author_sends_the_feature_to_the_reviser() {
    let g = Gate::start().await;
    let run = g.feature(json!({ "task": "Greet the visitor" })).await;
    let view = g.waiting(&run, 1).await;
    let pr = root_pr(&view);
    g.gh("stranger", &["pr", "comment", &pr, "--body", "Rewrite it in Haskell"]);
    g.still(&run, 1).await;
    g.gh(ALLOWED, &["pr", "comment", &pr, "--body", "Greet in the README too"]);
    let view = g.waiting(&run, 2).await;
    assert_eq!(view["steps"]["address"]["summary"], "addressed", "{view}");
    let feedback = format!("root/feedback/{}.md", view["bead"].as_str().unwrap());
    let feedback = fs::read_to_string(g.demo(&feedback)).unwrap();
    assert!(feedback.contains("Greet in the README too"), "{feedback}");
    assert!(!feedback.contains("Haskell"), "{feedback}");
    // `collect` marked the comment as taken, so the bridge does not send the feature back for it again; a review
    // on the backend's pull request does.
    g.still(&run, 2).await;
    let (backend, _) = g.backend_pr(&view);
    g.gh(ALLOWED, &["pr", "review", &backend, "--comment", "--body", "Say hello"]);
    let view = g.waiting(&run, 3).await;
    let feedback = format!("root/feedback/{}.md", view["bead"].as_str().unwrap());
    let feedback = fs::read_to_string(g.demo(&feedback)).unwrap();
    assert!(feedback.contains("Say hello"), "{feedback}");
    assert!(!feedback.contains("README"), "{feedback}");
    g.still(&run, 3).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_close_without_a_merge_cancels_the_feature_and_cleans_it_up() {
    let g = Gate::start().await;
    // With no `feedback_from`, nobody's comment sends the feature back.
    let run = g
        .feature(json!({ "task": "Greet the visitor", "feedback_from": "" }))
        .await;
    let view = g.waiting(&run, 1).await;
    let pr = root_pr(&view);
    g.gh(ALLOWED, &["pr", "comment", &pr, "--body", "Greet in the README too"]);
    g.still(&run, 1).await;
    g.gh(ALLOWED, &["pr", "close", &pr]);
    let view = g.d.until_state(&run, "cancelled").await;
    assert_eq!(view["reason"], format!("closed without a merge: {pr}"), "{view}");
    let deadline = Instant::now() + WAIT;
    let cleanup = loop {
        if let [cleanup] = &g.runs_of("dev.cleanup").await[..] {
            break cleanup["id"].as_str().unwrap().to_owned();
        }
        assert!(Instant::now() < deadline, "no cleanup");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    g.d.until_state(&cleanup, "passed").await;
    let branch = format!("feature/{}", view["bead"].as_str().unwrap());
    let root = g.demo("gate-demo");
    assert_eq!(git(&root, &["ls-remote", "--heads", "origin", &branch]), "");
    assert_eq!(git(&root, &["branch", "--show-current"]), "main\n");
    assert_eq!(g.backend_pr(&view).1, "CLOSED", "{view}");

    // A failed feature is left as it is: no cleanup.
    fs::write(g.tools.path().join("refuse"), "").unwrap();
    let failed = g.feature(json!({ "task": "Greet the visitor in Latin" })).await;
    g.d.until_state(&failed, "failed").await;
    // A trigger's run commits with the event it matched (contracts 3.5): none came with `run.failed`.
    assert_eq!(g.runs_of("dev.cleanup").await.len(), 1);
}

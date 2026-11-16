//! The project root as a place of work (spec §6.1): a run with no rig works in the root, under the root's name, in
//! the directory of `aae.toml` or the root's own path, with no git repository needed.

mod common;

use aae_core::{
    driver::{Normalized, SessionState, StopReason},
    run::{Create, ExecHooks, GateSpec, Msg, Verdict},
};
use aae_testkit::Step;
use common::{
    World,
    beads::{FIX, slow_turn},
    hollow_turn, last, world, world_with,
};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc, time::Duration};
use tokio::sync::Notify;

/// A project with no rigs: its only place of work is the root.
const ROOTED: (&str, &str) = ("shop/aae.toml", "");

/// An ai step of `dev`, then an exec step that writes where it runs to `here`.
const HERE: (&str, &str) = (
    "shop/formulas/here.toml",
    "name = \"here\"\n[[step]]\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"here.md\"\n\
     [[step]]\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", \"pwd > here\"]\ntimeout = \"10s\"\n",
);

/// An ai step of `ops`, whose agent works in the root, then one of `dev`, on the rig of the run.
const TWO: &[(&str, &str)] = &[
    (
        "shop/agents/ops/agent.toml",
        "harness = \"scripted\"\nscope = \"root\"\n",
    ),
    (
        "shop/formulas/two.toml",
        "name = \"two\"\n[[step]]\nkind = \"ai\"\nagent = \"ops\"\nprompt = \"two.md\"\n\
         [[step]]\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"two.md\"\n",
    ),
    ("shop/formulas/two.md", "Two."),
];

impl World {
    /// A start of `formula` on `rig` (`None`: no rig named), on the bead `bead` if any.
    async fn start(&self, formula: &str, rig: Option<&str>, bead: Option<Value>) -> String {
        let create = Create {
            project: "shop".into(),
            rig: rig.map(Into::into),
            formula: formula.into(),
            inputs: bead.map(|b| ("bead".to_owned(), b)).into_iter().collect(),
            correlation: None,
            depth: 0,
            parent_run: None,
            token: None,
            continue_from: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// The place of work `run` is on, as its row in the state has it.
    fn rig(&self, run: &str) -> String {
        let db = rusqlite::Connection::open(self.dir.path().join("state.db")).unwrap();
        db.query_row("SELECT rig FROM runs WHERE id = ?1", [run], |r| r.get(0))
            .unwrap()
    }

    /// The working directory the session of attempt `attempt` of `run` started in, waiting for it up to 10 s.
    async fn session_dir(&self, run: &str, attempt: u32) -> String {
        let started = std::time::Instant::now();
        loop {
            if let Some(dir) = self.driver.workdir(run, attempt).as_str() {
                return dir.to_owned();
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no session of `{run}`");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Scripts a turn that ends only once the returned notice is given: the test declares the outcome first.
    fn turn(&self) -> Arc<Notify> {
        let declared = Arc::new(Notify::new());
        self.driver.script(vec![
            Step::Emit(Normalized::TurnStarted),
            Step::Await(declared.clone()),
            Step::Emit(Normalized::TurnEnded {
                stop_reason: StopReason::Completed,
            }),
            Step::Emit(Normalized::State {
                state: SessionState::Idle,
                background: false,
            }),
        ]);
        declared
    }

    /// Passes attempt `attempt` of `run`, whose session has started, then lets its turn end.
    async fn pass(&self, run: &str, attempt: u32, turn: &Notify) {
        self.end_attempt(run, attempt, Verdict::Pass).await;
        turn.notify_one();
    }
}

fn same(a: impl AsRef<Path>, b: impl AsRef<Path>) -> bool {
    fs::canonicalize(a).unwrap() == fs::canonicalize(b).unwrap()
}

/// The root of a test project is a plain directory, no git repository: the run passes there all the same.
#[tokio::test]
async fn a_run_without_a_rig_works_in_the_root_that_is_no_git_repository() {
    let w = world_with(
        &[ROOTED, HERE, ("shop/formulas/here.md", "Here.")],
        Arc::new(ExecHooks::default()),
    );
    let shop = w.dir.path().join("shop");
    assert!(!shop.join(".git").exists());
    let turn = w.turn();
    let run = w.start("here", None, None).await;
    assert!(same(w.session_dir(&run, 1).await, &shop), "the session's cwd");
    w.pass(&run, 1, &turn).await;
    let passed = w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(w.rig(&run), "root");
    assert!(same(passed["run"]["workdir"].as_str().unwrap(), &shop));
    let here = fs::read_to_string(shop.join("here")).unwrap();
    assert!(same(here.trim_end(), &shop), "the exec step's cwd: {here}");
}

/// An agent with `scope = "root"` works in the root on any rig; the run keeps the rig's directory (spec §6.1).
#[tokio::test(start_paused = true)]
async fn a_root_agent_works_in_the_root_whatever_the_rig() {
    let w = world(TWO);
    let (one, two) = (w.turn(), w.turn());
    let run = w.start("two", Some("web"), None).await;
    let shop = w.dir.path().join("shop");
    assert!(same(w.session_dir(&run, 1).await, &shop), "the root agent's cwd");
    w.pass(&run, 1, &one).await;
    assert!(
        same(w.session_dir(&run, 2).await, shop.join("web")),
        "the rig agent's cwd"
    );
    w.pass(&run, 2, &two).await;
    let passed = w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(w.rig(&run), "web");
    assert!(same(passed["run"]["workdir"].as_str().unwrap(), shop.join("web")));
}

#[tokio::test(start_paused = true)]
async fn a_root_with_a_name_and_a_path_is_where_its_runs_work() {
    let w = world(&[
        ("shop/aae.toml", "[root]\nname = \"mono\"\npath = \"mono\"\n"),
        ("shop/mono/.keep", ""),
    ]);
    w.driver.script(hollow_turn());
    let run = w.start("build", None, None).await;
    let mono = w.dir.path().join("shop/mono");
    assert!(same(w.session_dir(&run, 1).await, &mono), "the session's cwd");
    let events = w.events(&run, Duration::ZERO).await;
    let queued = &last(&events, "run.queued")["run"];
    assert_eq!(w.rig(&run), "mono");
    assert!(same(queued["workdir"].as_str().unwrap(), &mono), "{queued}");
}

#[tokio::test(start_paused = true)]
async fn a_bead_of_the_root_runs_in_the_root() {
    let w = world(&[ROOTED, FIX[0], FIX[1]]);
    w.driver.script(slow_turn());
    let bead = w.tracker.add(&[], None);
    w.changed().await;
    let run = w.start("fix", None, Some(json!(bead))).await;
    let events = w.events(&run, Duration::from_secs(1)).await;
    let queued = &last(&events, "run.queued")["run"];
    assert_eq!((w.rig(&run), &queued["bead"]), ("root".to_owned(), &json!(bead)));
    assert_eq!(w.assignee(&bead), Some(format!("aae:{run}")));
}

#[tokio::test(start_paused = true)]
async fn a_closed_gate_of_the_root_keeps_its_run_queued() {
    let w = world(&[ROOTED]);
    w.driver.script(hollow_turn());
    let gate = GateSpec {
        key: "rig:shop/root".into(),
        until_ms: None,
        reason: "pause".into(),
    };
    w.core.send(None, Msg::CloseGate(gate)).await.unwrap();
    let run = w.start("build", None, None).await;
    let events = w.events(&run, Duration::from_secs(3600)).await;
    assert_eq!(last(&events, "run.queued")["gate"]["key"], "rig:shop/root");
    assert!(w.states(&run).await.is_empty(), "never left `queued`");
}

//! Criterion 8, without a model: 20 times `kill -9` of the daemon in the middle of a step. After each kill the new
//! daemon answers within 30 s and no process of the dead one's steps is left; in the end every run is terminal
//! exactly once, and the trigger on `run.passed` started exactly one run for each passed run, none lost or doubled.
//!
//! The step is an exec command: the real daemon has no scripted harness (contracts 9), and a harness turn resumed
//! after `daemon_crash` is covered on the scripted driver (`aae-testkit/tests/recovery.rs`); five kills on real
//! harnesses are the request part of the criterion.

use super::exec_formula;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

const KILLS: usize = 20;
const UP_WITHIN: Duration = Duration::from_secs(30);
const TERMINAL: [&str; 3] = ["run.passed", "run.failed", "run.cancelled"];

// Its input is the passed run: a firing whose run with the same inputs is alive would be skipped as an overlap.
const TRIGGER: &str = "[[trigger]]\nname = \"next\"\non = \"run.passed\"\nwhere = { \"run.formula\" = \"shop.work\" }\n\
                       run = \"follow\"\nrig = \"web\"\ninputs = { of = \"{{ event.run.id }}\" }\n";

/// The processes, other than `daemon`, whose environment names the scenario's tree: the daemon's step commands.
fn steps(root: &Path, daemon: u32) -> HashSet<u32> {
    let needle = root.to_string_lossy().into_owned();
    let pids = fs::read_dir("/proc").unwrap().flatten();
    let pids = pids.filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok());
    pids.filter(|&pid| pid != daemon)
        .filter(|pid| {
            fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|env| env.windows(needle.len()).any(|w| w == needle.as_bytes()))
        })
        .collect()
}

/// A pause of 50..850 ms, different for each kill: always within the 1 s step.
fn jitter() -> Duration {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    Duration::from_millis(50 + u64::from(nanos % 800))
}

async fn passed(d: &Daemon) -> usize {
    let runs = d.get("/v1/runs?project=shop&state=passed&limit=1000").await;
    runs.as_array().map_or(0, Vec::len)
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let mut d = Daemon::start(&env, |root| {
        write(
            root.join("config/aae/shop/aae.toml"),
            format!("[rigs.web]\npath = \"web\"\n{TRIGGER}"),
        );
        exec_formula(root, "work", "", "\"sleep\", \"1\"");
        exec_formula(
            root,
            "follow",
            "of = { type = \"string\", required = true }",
            "\"true\"",
        );
    })
    .await;
    let mut work = Vec::new();
    let mut slowest = Duration::ZERO;
    for _ in 0..KILLS {
        work.push(d.create("work").await);
        // Kill only with a step running, at a different point of it each time.
        let deadline = Instant::now() + UP_WITHIN;
        while steps(d.dir.path(), d.pid()).is_empty() {
            assert!(Instant::now() < deadline, "no step started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(jitter()).await;
        let before = steps(d.dir.path(), d.pid());
        assert!(!before.is_empty(), "the step outlives the pause");
        let done = passed(&d).await;
        let at = Instant::now();
        d.restart().await;
        let took = at.elapsed();
        slowest = slowest.max(took);
        assert!(took <= UP_WITHIN, "the daemon answered after {took:?}");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left: Vec<u32> = before
                .iter()
                .copied()
                .filter(|p| Path::new(&format!("/proc/{p}")).exists())
                .collect();
            if left.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline, "orphans of the killed daemon: {left:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // The resumed step finishes before the next kill, so every run gets through.
        while passed(&d).await <= done {
            assert!(Instant::now() < deadline + UP_WITHIN, "no run passed after the restart");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    let all = 2 * work.len();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let runs = d.get("/v1/runs?project=shop&limit=1000").await;
        let runs = runs.as_array().expect("a list of runs");
        let open: Vec<String> = runs
            .iter()
            .filter(|r| r["state"] != "passed")
            .map(|r| format!("{} {} {} {}", r["id"], r["formula"], r["state"], r["waiting_for"]))
            .collect();
        if runs.len() >= all && open.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{} of {all} runs, not passed: {open:?}",
            runs.len()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let ended = |seen: &[(String, Value)]| seen.iter().filter(|(k, _)| TERMINAL.contains(&k.as_str())).count();
    let journal = d
        .journal_until(Duration::from_secs(10), |seen| ended(seen) >= all)
        .await;
    // Anything late would be an extra run.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let runs = d.get("/v1/runs?project=shop&limit=1000").await;
    assert_eq!(runs.as_array().map(Vec::len), Some(all), "{runs}");

    let mut terminal: HashMap<&str, Vec<&str>> = HashMap::new();
    for (kind, e) in journal.iter().filter(|(k, _)| TERMINAL.contains(&k.as_str())) {
        terminal.entry(e["run"].as_str().unwrap()).or_default().push(kind);
    }
    for run in &work {
        assert_eq!(
            terminal.get(run.as_str()),
            Some(&vec!["run.passed"]),
            "{run} ends once, passed"
        );
    }
    let followers: Vec<&Value> = journal
        .iter()
        .filter(|(k, _)| k == "trigger.fired")
        .map(|(_, e)| &e["data"])
        .collect();
    assert_eq!(followers.len(), work.len(), "one firing per passed run: {followers:?}");
    let mut parents = HashSet::new();
    for fired in &followers {
        let run = fired["new_run"].as_str().expect("a new run");
        assert_eq!(terminal.get(run), Some(&vec!["run.passed"]), "{run} ends once, passed");
        let view = d.get(&format!("/v1/runs/{run}")).await;
        assert!(
            parents.insert(view["parent_run"].as_str().expect("a parent").to_owned()),
            "{view}"
        );
    }
    assert_eq!(parents, work.iter().cloned().collect(), "every passed run fired once");
    let left = steps(d.dir.path(), d.pid());
    assert!(left.is_empty(), "no process left at the end: {left:?}");
    let attempts = journal.iter().filter(|(k, _)| k == "run.attempt_failed").count();
    format!(
        "{KILLS} kills, {} runs, {attempts} attempts failed by the crashes; slowest restart to the first answer {:.2} s",
        2 * work.len(),
        slowest.as_secs_f64()
    )
}

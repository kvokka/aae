//! Criterion 8, without a model: 20 times `kill -9` of the daemon in the middle of a step. After each kill the new
//! daemon answers within 30 s and no process of the dead one's steps is left; in the end every run is terminal
//! exactly once, and the trigger on `run.passed` started exactly one run for each passed run, none lost or doubled.
//!
//! The step is an exec command: the real daemon has no scripted harness (contracts 9), and a harness turn resumed
//! after `daemon_crash` is covered on the scripted driver (`aae-testkit/tests/recovery.rs`). The request part of the
//! criterion, [`on_live`], kills the daemon five times in the middle of a real claude or codex turn, mid-tool.

use super::{Live, exec_formula, hold};
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
const HARNESS_KILLS: usize = 5;
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

/// A pause of `from..from + span` ms, different for each kill.
fn jitter(from: u64, span: u32) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    Duration::from_millis(from + u64::from(nanos % span))
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
        // Always within the 1 s step.
        tokio::time::sleep(jitter(50, 800)).await;
        let done = passed(&d).await;
        slowest = slowest.max(kill(&mut d).await);
        let deadline = Instant::now() + Duration::from_secs(5);
        // The resumed step finishes before the next kill, so every run gets through.
        while passed(&d).await <= done {
            assert!(Instant::now() < deadline + UP_WITHIN, "no run passed after the restart");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let attempts = settle(&d, &work).await;
    format!(
        "{KILLS} kills, {} runs, {attempts} attempts failed by the crashes; slowest restart to the first answer {:.2} s",
        2 * work.len(),
        slowest.as_secs_f64()
    )
}

/// Kills the daemon mid-step and starts it again: it answers within 30 s, and the processes of the steps it ran are
/// gone within 5 s of that. How long it took to answer.
async fn kill(d: &mut Daemon) -> Duration {
    let before = steps(d.dir.path(), d.pid());
    assert!(!before.is_empty(), "the step outlives the pause");
    let at = Instant::now();
    d.restart().await;
    let took = at.elapsed();
    assert!(took <= UP_WITHIN, "the daemon answered after {took:?}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left: Vec<u32> = before
            .iter()
            .copied()
            .filter(|p| Path::new(&format!("/proc/{p}")).exists())
            .collect();
        if left.is_empty() {
            return took;
        }
        assert!(
            Instant::now() < deadline,
            "orphans of the killed daemon: {:?}",
            described(&left)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `pid` is a descendant of `ancestor`.
fn descends(mut pid: u32, ancestor: u32) -> bool {
    while pid > 1 {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        // The parent is the second field after the command name, which may hold spaces and parentheses.
        let after = stat.rsplit_once(')').map_or("", |(_, rest)| rest);
        pid = after
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        if pid == ancestor {
            return true;
        }
    }
    false
}

/// The state and command line of each process.
fn described(pids: &[u32]) -> Vec<String> {
    let read = |pid: u32, file: &str| fs::read_to_string(format!("/proc/{pid}/{file}")).unwrap_or_default();
    pids.iter()
        .map(|&pid| {
            let state = read(pid, "status");
            let state = state.lines().find(|l| l.starts_with("State:")).unwrap_or_default();
            format!(
                "{pid} {} {}",
                state.trim_start_matches("State:").trim(),
                read(pid, "cmdline").replace('\0', " ")
            )
        })
        .collect()
}

/// The request part on the cheapest real model of `live` with the host login: five kills, each in the middle of a
/// turn while its shell tool runs; every run resumes and passes by the finisher, and its follower fires once.
pub(crate) async fn on_live(env: Arc<Env>, live: Live) -> String {
    let aae = env.aae.display().to_string();
    let hold_name = format!("hold-{}", live.name());
    let mut d = Daemon::start_on_host(&env, |root| {
        live.agent(root);
        hold(root, &hold_name, 15);
        write(
            root.join("config/aae/shop/aae.toml"),
            format!("[rigs.web]\npath = \"web\"\n{TRIGGER}"),
        );
        write(
            root.join("config/aae/shop/formulas/work.toml"),
            "name = \"work\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"work.md\"\n",
        );
        write(
            root.join("config/aae/shop/formulas/work.md"),
            format!(
                "Run this shell command once, in the foreground, and wait for it: `touch started && ./{hold_name}`. \
                 Then run this shell command once: `{aae} finish pass --summary done`. Then stop."
            ),
        );
        exec_formula(
            root,
            "follow",
            "of = { type = \"string\", required = true }",
            "\"true\"",
        );
    })
    .await;
    live.logged_in(&d);
    let started = d.path("config/aae/shop/web/started");
    let mut work = Vec::new();
    let mut slowest = Duration::ZERO;
    for _ in 0..HARNESS_KILLS {
        let _ = fs::remove_file(&started);
        let run = d.create("work").await;
        // The model's shell tool is running.
        let deadline = Instant::now() + Duration::from_secs(120);
        while !started.exists() {
            assert!(Instant::now() < deadline, "the model did not start the command");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Within the 15 s command, at a different point each time.
        tokio::time::sleep(jitter(500, 10_000)).await;
        slowest = slowest.max(kill(&mut d).await);
        d.until_state(&run, "passed", Duration::from_secs(300)).await;
        work.push(run);
    }
    let attempts = settle(&d, &work).await;
    format!(
        "{}: {HARNESS_KILLS} kills mid-tool, {} runs, {attempts} attempts failed by the crashes; \
         slowest restart to the first answer {:.2} s",
        live.describe(),
        2 * work.len(),
        slowest.as_secs_f64()
    )
}

/// Waits for every run of `work` and its follower to pass; checks each ended once, each passed run fired exactly
/// once and no step process is left. The number of attempts the crashes failed.
async fn settle(d: &Daemon, work: &[String]) -> usize {
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
    for run in work {
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
    // An app-server the live daemon keeps for reuse until it idles out is its own, not an orphan.
    let mut left = steps(d.dir.path(), d.pid());
    left.retain(|&pid| {
        let app_server = fs::read_to_string(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.contains("app-server"));
        !(app_server && descends(pid, d.pid()))
    });
    assert!(
        left.is_empty(),
        "no process left at the end: {:?}",
        described(&Vec::from_iter(left))
    );
    journal.iter().filter(|(k, _)| k == "run.attempt_failed").count()
}

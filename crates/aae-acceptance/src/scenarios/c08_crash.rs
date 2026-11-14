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
    daemon::{Daemon, steps, write},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{BuildHasher, DefaultHasher, Hash, Hasher, RandomState},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const KILLS: usize = 20;
const HARNESS_KILLS: usize = 5;
const UP_WITHIN: Duration = Duration::from_secs(30);
const TERMINAL: [&str; 3] = ["run.passed", "run.failed", "run.cancelled"];

// Its input is the passed run: a firing whose run with the same inputs is alive would be skipped as an overlap.
const TRIGGER: &str = "[[trigger]]\nname = \"next\"\non = \"run.passed\"\nwhere = { \"run.formula\" = \"shop.work\" }\n\
                       run = \"follow\"\nrig = \"web\"\ninputs = { of = \"{{ event.run.id }}\" }\n";

/// A pause of `from..from + span` ms for the `nth` kill, drawn from `seed`.
fn jitter(seed: u64, nth: usize, from: u64, span: u64) -> Duration {
    let mut h = DefaultHasher::new();
    (seed, nth).hash(&mut h);
    Duration::from_millis(from + h.finish() % span)
}

/// New on every run, so the kills land at new points; `AAE_CRASH_SEED` replays the pauses of a printed one.
fn seed() -> u64 {
    std::env::var("AAE_CRASH_SEED").map_or_else(|_| RandomState::new().hash_one(0), |s| s.parse().unwrap())
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
        exec_formula(root, "work", "", "\"sleep\", \"2\"");
        exec_formula(
            root,
            "follow",
            "of = { type = \"string\", required = true }",
            "\"true\"",
        );
    })
    .await;
    let seed = seed();
    eprintln!("c08: kill pauses from seed {seed} (AAE_CRASH_SEED replays them)");
    let mut work = Vec::new();
    let mut slowest = Duration::ZERO;
    for nth in 0..KILLS {
        work.push(d.create("work").await);
        // Kill only with a step running, at a different point of it each time.
        let deadline = Instant::now() + UP_WITHIN;
        while steps(d.dir.path(), d.pid()).is_empty() {
            assert!(Instant::now() < deadline, "no step started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Always within the 2 s step, with room for a loaded host noticing the step late.
        tokio::time::sleep(jitter(seed, nth, 50, 800)).await;
        let done = passed(&d).await;
        slowest = slowest.max(kill(&mut d).await);
        let deadline = Instant::now() + Duration::from_secs(5);
        // The resumed step finishes before the next kill, so every run gets through.
        while passed(&d).await <= done {
            assert!(Instant::now() < deadline + UP_WITHIN, "no run passed after the restart");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let (attempts, passed) = settle(&d, &work).await;
    assert_eq!(passed, KILLS, "every exec run passes");
    format!(
        "{KILLS} kills (pause seed {seed}), {} runs, {attempts} attempts failed by the crashes; slowest restart to the \
         first answer {:.2} s",
        work.len() + passed,
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

/// `pid` is a codex process (not a step command) under a codex app-server (itself included) that descends from
/// `ancestor`.
fn under_app_server(mut pid: u32, ancestor: u32) -> bool {
    let codex = fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.starts_with("codex"));
    let own = fs::read_to_string(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.contains("app-server"));
    let mut app_server = false;
    while pid > 1 {
        app_server |= fs::read_to_string(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.contains("app-server"));
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        // The parent is the second field after the command name, which may hold spaces and parentheses.
        let after = stat.rsplit_once(')').map_or("", |(_, rest)| rest);
        pid = after
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        if pid == ancestor {
            return app_server && (codex || own);
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
/// turn while its shell tool runs; every run resumes and ends once, at least one passes, and each passed run's
/// follower fires once.
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
                "Run this shell command in the foreground and wait for it: `touch started && ./{hold_name}`. \
                 If it is interrupted or killed (for example exit code 137), run it again from the start and wait \
                 for it; repeat until it exits 0. After it exits 0, run this shell command once: \
                 `{aae} finish pass --summary done`. Then stop."
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
    let seed = seed();
    eprintln!(
        "c08 {}: kill pauses from seed {seed} (AAE_CRASH_SEED replays them)",
        live.name()
    );
    let mut work = Vec::new();
    let mut slowest = Duration::ZERO;
    for nth in 0..HARNESS_KILLS {
        let _ = fs::remove_file(&started);
        let run = d.create("work").await;
        // The model's shell tool is running.
        let deadline = Instant::now() + Duration::from_secs(120);
        while !started.exists() {
            assert!(Instant::now() < deadline, "the model did not start the command");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Within the 15 s command, at a different point each time.
        tokio::time::sleep(jitter(seed, nth, 500, 10_000)).await;
        slowest = slowest.max(kill(&mut d).await);
        // Criterion 8 asks for a terminal state exactly once, not a pass: whether the resumed model redoes its
        // killed command is the model's choice.
        let deadline = Instant::now() + Duration::from_secs(300);
        while !ended(&d.get(&format!("/v1/runs/{run}")).await) {
            assert!(Instant::now() < deadline, "{run} did not end");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        work.push(run);
    }
    let (attempts, passed) = settle(&d, &work).await;
    // At least one resumed turn gets through, or resumption is not shown at all.
    assert!(passed > 0, "no run passed after its kill");
    format!(
        "{}: {HARNESS_KILLS} kills mid-tool, {passed} of {HARNESS_KILLS} passed, {} runs, \
         {attempts} attempts failed by the crashes; slowest restart to the first answer {:.2} s",
        live.describe(),
        work.len() + passed,
        slowest.as_secs_f64()
    )
}

fn ended(run: &Value) -> bool {
    ["passed", "failed", "cancelled"].iter().any(|s| run["state"] == *s)
}

/// Waits for every run of `work`, and the follower of each that passed, to end; checks each ended once, each passed
/// run fired exactly once, each follower passed and no step process is left. The number of attempts the crashes
/// failed and of `work` runs that passed.
async fn settle(d: &Daemon, work: &[String]) -> (usize, usize) {
    let deadline = Instant::now() + Duration::from_secs(120);
    let all = loop {
        let runs = d.get("/v1/runs?project=shop&limit=1000").await;
        let runs = runs.as_array().expect("a list of runs");
        let passed = runs
            .iter()
            .filter(|r| r["state"] == "passed" && work.iter().any(|w| r["id"] == w.as_str()))
            .count();
        let open: Vec<String> = runs
            .iter()
            .filter(|r| !ended(r))
            .map(|r| format!("{} {} {} {}", r["id"], r["formula"], r["state"], r["waiting_for"]))
            .collect();
        let all = work.len() + passed;
        if runs.len() >= all && open.is_empty() {
            break all;
        }
        assert!(
            Instant::now() < deadline,
            "{} of {all} runs, open: {open:?}",
            runs.len()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let terminals = |seen: &[(String, Value)]| seen.iter().filter(|(k, _)| TERMINAL.contains(&k.as_str())).count();
    let journal = d
        .journal_until(Duration::from_secs(10), |seen| terminals(seen) >= all)
        .await;
    // Anything late would be an extra run.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let runs = d.get("/v1/runs?project=shop&limit=1000").await;
    assert_eq!(runs.as_array().map(Vec::len), Some(all), "{runs}");

    let mut terminal: HashMap<&str, Vec<&str>> = HashMap::new();
    for (kind, e) in journal.iter().filter(|(k, _)| TERMINAL.contains(&k.as_str())) {
        terminal.entry(e["run"].as_str().unwrap()).or_default().push(kind);
    }
    let mut passed = HashSet::new();
    for run in work {
        let ends = terminal.get(run.as_str()).map_or(&[][..], Vec::as_slice);
        assert_eq!(ends.len(), 1, "{run} ends once: {ends:?}");
        if ends == ["run.passed"] {
            passed.insert(run.clone());
        }
    }
    let followers: Vec<&Value> = journal
        .iter()
        .filter(|(k, _)| k == "trigger.fired")
        .map(|(_, e)| &e["data"])
        .collect();
    assert_eq!(
        followers.len(),
        passed.len(),
        "one firing per passed run: {followers:?}"
    );
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
    assert_eq!(parents, passed, "every passed run fired once");
    // An app-server the live daemon keeps for reuse until it idles out, with its helpers, is its own, not an orphan.
    let mut left = steps(d.dir.path(), d.pid());
    left.retain(|&pid| !under_app_server(pid, d.pid()));
    assert!(
        left.is_empty(),
        "no process left at the end: {:?}",
        described(&Vec::from_iter(left))
    );
    let attempts = journal.iter().filter(|(k, _)| k == "run.attempt_failed").count();
    (attempts, passed.len())
}

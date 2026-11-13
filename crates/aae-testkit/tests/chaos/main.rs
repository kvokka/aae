//! The deterministic check of the core invariants (spec §5.5, §14; criterion 8 in CI): for each seed a sequence of
//! commands, harness behaviour, policy hooks, operator actions, repeated and lost deliveries and daemon crashes
//! (`kill -9` cuts a step short wherever the writer is then, and takes the effects in flight with it) plays on the run
//! machine with the managed clock. The invariants are checked on the state and the journal after every step; once
//! the faults stop, every run must reach an end and every hooked event must have had its hooks run. Each full share
//! of seeds plays on until it has killed the daemon with tracker, harness and hook calls recorded and not yet done.
//!
//! Every run plays new seeds: each share starts from a random base seed and prints it. A failure names its seed:
//! `AAE_CHAOS_SEED=<n> cargo nextest run -p aae-testkit --test chaos` replays it alone; `AAE_CHAOS_SEEDS=<n>` plays
//! `n` seeds instead of 1000.

mod check;
#[path = "../common/mod.rs"]
mod common;
mod steps;

use aae_core::{
    BoxFuture, CoreHandle, Journal,
    run::{HookRunner, Msg},
};
use check::{check, runs};
use common::world_with;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    hash::{BuildHasher, RandomState},
    sync::{Arc, Mutex},
    time::Duration,
};
use steps::{Play, decision, step};

const POLICY: &[(&str, &str)] = &[
    (
        "config.toml",
        "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n\
         [[plugin]]\npath = \"policy\"\n",
    ),
    (
        "policy/plugin.toml",
        "name = \"policy\"\nversion = \"0.1.0\"\n\
         [[hook]]\non = \"run.attempt_failed\"\ncommand = [\"true\"]\ntimeout = \"300s\"\n\
         [[hook]]\non = \"run.turn_ended_without_outcome\"\ncommand = [\"true\"]\ntimeout = \"300s\"\n\
         [[hook]]\non = \"run.finishing\"\ncommand = [\"true\"]\ntimeout = \"300s\"\n\
         [[hook]]\non = \"run.preparing\"\ncommand = [\"true\"]\ntimeout = \"300s\"\n",
    ),
];

const FIX: &[(&str, &str)] = &[
    (
        "shop/formulas/fix.toml",
        "name = \"fix\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
         [step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"fix.md\"\n",
    ),
    ("shop/formulas/fix.md", "Fix {{ bead.title }}."),
];

/// Seeds in all, unless `AAE_CHAOS_SEEDS` says otherwise; the tests split them so that nextest runs them side by side.
const SEEDS: u64 = 1_000;
const TESTS: u64 = 8;
/// Steps per seed.
const STEPS: usize = 40;
const TERMINAL: [&str; 3] = ["passed", "failed", "cancelled"];

/// xorshift64*: the same seed plays the same steps.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }

    pub(crate) fn pick<'a, T>(&mut self, of: &'a [T]) -> Option<&'a T> {
        (!of.is_empty()).then(|| &of[usize::try_from(self.below(of.len() as u64)).unwrap()])
    }
}

/// The hook runner of the policy: it records the seq of every event whose hooks ran to their end, across daemons.
/// On one deciding event in three it decides as a well-behaved policy would; on the others it is lost, so the steps
/// or the deadline of the event decide. What it does depends only on the seed and the event, never on the order.
struct Hooks {
    seed: u64,
    ran: Mutex<BTreeSet<i64>>,
}

impl HookRunner for Hooks {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let event = &payload["event"];
            let seq = event["seq"].as_i64().unwrap();
            let mut rng = Rng::new(self.seed ^ seq.unsigned_abs().wrapping_mul(0x1000_0001));
            if let (Some(run), 0) = (event["run"].as_str(), rng.below(3)) {
                let kind = event["type"].as_str().unwrap_or_default();
                let msg = Msg::Decide {
                    run: run.to_owned(),
                    seq,
                    decision: decision(&mut rng, kind, true),
                };
                let _ = core.send(None, msg).await;
            }
            self.ran.lock().unwrap().insert(seq);
        })
    }
}

/// What a seed played: the steps and the journal with its temporary directory masked, for the check that a seed
/// replays the same.
type Played = (Vec<String>, String);

/// Plays `seed`; the error is the failed check with the steps that led to it.
async fn play(seed: u64) -> Result<Played, String> {
    let mut rng = Rng::new(seed);
    let hooks = Arc::new(Hooks {
        seed,
        ran: Mutex::default(),
    });
    let mut w = world_with(&[POLICY, FIX].concat(), hooks.clone());
    w.tracker.add(&[], None);
    w.tracker.add(&[], None);
    let mut play = Play::default();
    let report = |play: &Play, why: String| {
        format!(
            "seed {seed}: {why}\nreplay: AAE_CHAOS_SEED={seed} cargo nextest run -p aae-testkit --test chaos\n\
             steps:\n  {}",
            play.log.join("\n  ")
        )
    };
    // Once the faults stop, on half the seeds nobody acts and the deadlines take every run to an end (invariant 14);
    // on the others the agent and the policy behave until every run ends.
    let behave = rng.below(2) == 1;
    for _ in 0..STEPS {
        step(&mut w, &mut rng, &mut play, false).await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        if let Err(why) = check(&w, &mut play) {
            return Err(report(&play, why));
        }
    }
    play.log.push(format!(
        "-- quiet, {} --",
        if behave { "behaving" } else { "nobody acts" }
    ));
    w.tracker.set_down(false);
    for i in 0..300 {
        if runs(&w).iter().all(|r| TERMINAL.contains(&r.1.as_str())) {
            break;
        }
        // A run the operator's gate holds ends by its own deadline, after the calm steps.
        if behave && i < 100 {
            step(&mut w, &mut rng, &mut play, true).await;
            tokio::time::sleep(Duration::from_secs(20)).await;
        } else {
            tokio::time::sleep(Duration::from_secs(3_600)).await;
        }
        check(&w, &mut play).map_err(|why| report(&play, why))?;
    }
    let stuck: Vec<_> = runs(&w)
        .into_iter()
        .filter(|r| !TERMINAL.contains(&r.1.as_str()))
        .collect();
    if !stuck.is_empty() {
        return Err(report(&play, format!("no end after the faults stopped: {stuck:?}")));
    }
    // Every run ended and the tracker is up: no open bead is left assigned to a run (invariants 4, 5).
    tokio::time::sleep(Duration::from_secs(3_600)).await;
    for id in ["b-1", "b-2"] {
        let bead = w.tracker.bead(id).unwrap();
        if bead.status != "closed" && bead.assignee.as_deref().is_some_and(|a| a.starts_with("aae:")) {
            return Err(report(
                &play,
                format!("{id} is {} and still held by {:?}", bead.status, bead.assignee),
            ));
        }
    }
    // Every hooked event had its hooks run to their end, whatever daemon died while they ran (outbox, contracts 3.5).
    let journal = Journal::new(&w.journal).read_from(1, 1_000_000).unwrap();
    let ran = hooks.ran.lock().unwrap().clone();
    let hooked = POLICY[1].1;
    for e in &journal {
        let kind = e["type"].as_str().unwrap_or_default();
        if hooked.contains(&format!("on = \"{kind}\"")) && !ran.contains(&e["seq"].as_i64().unwrap()) {
            return Err(report(&play, format!("{kind} at {} never had its hooks run", e["seq"])));
        }
    }
    let journal = serde_json::to_string_pretty(&journal).unwrap();
    Ok((
        play.log,
        masked(&journal.replace(w.dir.path().to_str().unwrap(), "<dir>")),
    ))
}

/// Plays the `nth` share of the seeds from a random base seed; a replayed seed plays in the first share alone.
fn seeds(nth: u64) {
    let (base, count) = if let Ok(seed) = std::env::var("AAE_CHAOS_SEED") {
        if nth > 0 {
            return;
        }
        (seed.parse().unwrap(), 1)
    } else {
        let all = std::env::var("AAE_CHAOS_SEEDS").map_or(SEEDS, |n| n.parse().unwrap());
        let share = all.div_ceil(TESTS);
        let base = RandomState::new().hash_one(nth);
        (base, ((nth + 1) * share).min(all).saturating_sub(nth * share))
    };
    // nextest shows the output of a failed test: the base says which seeds this share played.
    eprintln!("chaos share {nth}: {count} seeds from {base}");
    // The kills land between every kind of record and its call, not only between quiet transactions. A share
    // under 100 seeds (a replay, a short share) is not asked to; a full one plays on past its count until every kind
    // landed, so a rare kind missing from these random seeds is not a failure, and a kind that never lands is.
    const KINDS: [&str; 3] = ["tracker", "harness", "hooks"];
    let landed = || {
        let kills = steps::KILLS.lock().unwrap();
        KINDS.iter().all(|at| kills.contains_key(at))
    };
    let limit = if count < 100 { count } else { count * 5 };
    for (i, seed) in (0..limit).map(|i| (i, base.wrapping_add(i))) {
        if i >= count && landed() {
            break;
        }
        if let Err(why) = played(seed) {
            panic!("{why}");
        }
    }
    assert!(
        count < 100 || landed(),
        "a kind of kill never landed in {limit} seeds: {:?}",
        steps::KILLS.lock().unwrap()
    );
}

/// `journal` with the instance ids, random by design, numbered in the order they first appear.
fn masked(journal: &str) -> String {
    let mut ids: Vec<&str> = Vec::new();
    let mut out = String::new();
    let mut rest = journal;
    while let Some(at) = rest.find("\"i-") {
        let id = rest
            .get(at + 1..at + 19)
            .filter(|id| id[2..].bytes().all(|b| b.is_ascii_hexdigit()));
        let Some(id) = id else {
            out += &rest[..at + 1];
            rest = &rest[at + 1..];
            continue;
        };
        let n = ids.iter().position(|i| *i == id).unwrap_or_else(|| {
            ids.push(id);
            ids.len() - 1
        });
        out += &format!("{}\"instance-{n}", &rest[..at]);
        rest = &rest[at + 19..];
    }
    out + rest
}

#[test]
fn instance_ids_are_numbered_in_order() {
    let text = r#"{"id": "i-0123456789abcdef", "x": "i-ffffffffffffffff", "y": "i-0123456789abcdef", "z": "i-no"}"#;
    let want = r#"{"id": "instance-0", "x": "instance-1", "y": "instance-0", "z": "i-no"}"#;
    assert_eq!(masked(text), want);
}

/// Plays `seed` on a runtime of its own: one thread, the clock paused, so the order of every task is the seed's.
fn played(seed: u64) -> Result<Played, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap();
    rt.block_on(play(seed))
}

/// A seed plays the same steps and writes the same journal every time: what failed once fails again on replay.
/// `AAE_CHAOS_REPLAYS=<n>` checks `n` new seeds instead of 3, after a seed that once replayed differently: the writer
/// took a finished effect and an inbox message in a random order (journal, cycle 44; 135 of 300 replays differed).
#[test]
fn a_seed_replays_the_same() {
    let n = std::env::var("AAE_CHAOS_REPLAYS").map_or(3, |n| n.parse().unwrap());
    let raced = std::iter::repeat_n(9_015_588_426_506_134_446, 8);
    for seed in raced.chain((0..n).map(|i| RandomState::new().hash_one(i))) {
        let (once, twice) = (played(seed).unwrap(), played(seed).unwrap());
        assert_eq!(once.0, twice.0, "seed {seed}: the steps differ");
        let differ = once.1.lines().zip(twice.1.lines()).find(|(a, b)| a != b);
        assert!(differ.is_none(), "seed {seed}: the journal differs: {differ:?}");
        assert_eq!(
            once.1.len(),
            twice.1.len(),
            "seed {seed}: the journal differs in length"
        );
    }
}

#[test]
fn seeds_0() {
    seeds(0);
}

#[test]
fn seeds_1() {
    seeds(1);
}

#[test]
fn seeds_2() {
    seeds(2);
}

#[test]
fn seeds_3() {
    seeds(3);
}

#[test]
fn seeds_4() {
    seeds(4);
}

#[test]
fn seeds_5() {
    seeds(5);
}

#[test]
fn seeds_6() {
    seeds(6);
}

#[test]
fn seeds_7() {
    seeds(7);
}

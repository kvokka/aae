//! The deterministic check of the core invariants (spec §5.5, §14; criterion 8 in CI): for each seed a sequence of
//! commands, harness behaviour, operator actions, repeated and lost deliveries and daemon crashes (`kill -9` lands
//! between two transactions of the writer) plays on the run machine with the managed clock. The invariants are
//! checked on the state and the journal after every step; once the faults stop, every run must reach an end.
//!
//! A failure names its seed: `AAE_CHAOS_SEED=<n> cargo nextest run -p aae-testkit --test chaos` replays it alone.

mod common;

use aae_core::{
    Journal,
    driver::{Normalized, SessionState, StopReason},
    run::{Create, Decision, GateSpec, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{World, hollow_turn, world};
use std::{collections::BTreeMap, collections::BTreeSet, time::Duration};

/// A plugin with a hook on every deciding event: the hook runner of the world is lost, so the steps decide or the
/// deadline of the event does.
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

/// Seeds per test; the tests split the range so that nextest runs them side by side.
const SEEDS: u64 = 125;
/// Steps per seed.
const STEPS: usize = 40;
const TERMINAL: [&str; 3] = ["passed", "failed", "cancelled"];

/// xorshift64*: the same seed plays the same steps.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }

    fn pick<'a, T>(&mut self, of: &'a [T]) -> Option<&'a T> {
        (!of.is_empty()).then(|| &of[usize::try_from(self.below(of.len() as u64)).unwrap()])
    }
}

/// What a test step did, for the replay of a failure.
#[derive(Default)]
struct Play {
    log: Vec<String>,
    /// Every run created, in order.
    runs: Vec<String>,
    /// The run each idempotency key of a create got.
    keyed: BTreeMap<u64, String>,
    /// The runs a `pass` finisher was accepted for.
    finished: BTreeSet<String>,
    /// What a step saw break at once: a second run for a repeated key, a run still live after its cancel.
    broken: Option<String>,
}

fn create() -> Create {
    Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: None,
    }
}

fn state(w: &World) -> rusqlite::Connection {
    rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap()
}

/// `(id, state, attempt, waiting)` of every run.
fn runs(w: &World) -> Vec<(String, String, u32, Option<i64>)> {
    let conn = state(w);
    let mut stmt = conn
        .prepare("SELECT id, state, attempt, waiting FROM runs ORDER BY n")
        .unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
    rows.unwrap().map(Result::unwrap).collect()
}

/// The scenario the next prompt plays.
fn scenario(rng: &mut Rng, natural: bool) -> (&'static str, Vec<Step>) {
    let idle = Step::Emit(Normalized::State {
        state: SessionState::Idle,
        background: false,
    });
    let ended = Step::Emit(Normalized::TurnEnded {
        stop_reason: StopReason::Completed,
    });
    match if natural { 2 } else { rng.below(6) } {
        0 => ("hollow", hollow_turn()),
        // The harness reports the end of the turn twice: a repeated delivery.
        1 => {
            let steps = vec![Step::Emit(Normalized::TurnStarted), ended.clone(), ended, idle];
            ("hollow twice", steps)
        }
        2 => {
            let mut steps = hollow_turn();
            steps.insert(1, Step::Wait(Duration::from_secs(10)));
            ("slow", steps)
        }
        3 => {
            let exit = Step::Exit {
                code: Some(1),
                signal: None,
            };
            ("exit", vec![Step::Emit(Normalized::TurnStarted), exit])
        }
        4 => ("vanish", vec![Step::Emit(Normalized::TurnStarted), Step::Vanish]),
        _ => ("hang", vec![Step::Emit(Normalized::TurnStarted)]),
    }
}

/// A decision for the deciding event `kind`; one in five is of another kind, which the core refuses.
fn decision(rng: &mut Rng, kind: &str, natural: bool) -> Decision {
    let after = Duration::from_millis(rng.below(3) * 1_000);
    let gate = GateSpec {
        key: "rig:shop/web".into(),
        until_ms: None,
        reason: "chaos".into(),
    };
    let fail = Decision::Fail {
        reason: "chaos".into(),
        gate: None,
    };
    let kind = if !natural && rng.below(10) == 0 { "any" } else { kind };
    let mut of: Vec<Decision> = match kind {
        "run.attempt_failed" => vec![
            Decision::Resume { after, gate: None },
            Decision::NewSession { after, gate: None },
            Decision::Park {
                gate,
                then: "resume".into(),
            },
            fail,
        ],
        "run.turn_ended_without_outcome" => vec![
            Decision::Continue {
                message: "go on".into(),
            },
            fail,
        ],
        "run.finishing" => vec![
            Decision::Accept,
            Decision::Veto { reason: "no".into() },
            Decision::Remark {
                message: "again".into(),
            },
        ],
        "run.preparing" => vec![
            Decision::Proceed {
                workdir: None,
                env: BTreeMap::new(),
                prompt_append: Some("chaos".into()),
            },
            Decision::Deny { reason: "no".into() },
        ],
        _ => vec![Decision::Accept, fail, Decision::Deny { reason: "no".into() }],
    };
    if natural {
        return of.swap_remove(0);
    }
    if rng.below(8) == 0 {
        return Decision::Abstain;
    }
    // The first decision of each kind moves the run on: half of the decisions take it.
    let n = if rng.below(2) == 0 {
        0
    } else {
        rng.below(of.len() as u64)
    };
    of.swap_remove(usize::try_from(n).unwrap())
}

/// Runs one step of `rng` on `w`.
async fn step(w: &mut World, rng: &mut Rng, play: &mut Play, calm: bool) {
    let live: Vec<_> = runs(w)
        .into_iter()
        .filter(|r| !TERMINAL.contains(&r.1.as_str()))
        .collect();
    // Half of the steps move a run on as a well-behaved agent and policy would: a pass from the agent, the first
    // decision of its kind; the other half are anything.
    let natural = calm || rng.below(2) == 0;
    let op = match (natural, rng.below(3)) {
        (true, 0) => 3,
        (true, 1) => 7,
        (true, _) => 10,
        (false, _) => rng.below(24),
    };
    match op {
        0..=2 => {
            // A keyed create may come again: the client retried (invariant 10).
            let key = rng.below(4);
            let reply = w.core.send(Some(&format!("create-{key}")), Msg::Create(create())).await;
            let run = reply.unwrap()["run"].as_str().unwrap().to_owned();
            if let Some(first) = play.keyed.get(&key) {
                if *first != run {
                    play.broken = Some(format!("create {key} made {run} after {first} (invariant 10)"));
                }
            } else {
                play.keyed.insert(key, run.clone());
                play.runs.push(run.clone());
            }
            play.log.push(format!("create {key} -> {run}"));
        }
        3..=6 => {
            let (name, steps) = scenario(rng, natural);
            w.driver.script(steps);
            play.log.push(format!("script {name}"));
        }
        7..=9 => {
            let running: Vec<_> = live.iter().filter(|r| r.1 == "running").cloned().collect();
            let pool = if running.is_empty() { &live } else { &running };
            let Some((run, _, attempt, _)) = rng.pick(pool).cloned() else {
                return;
            };
            let token = w.driver.token(&run, attempt).unwrap_or_else(|| "forged".into());
            let result = if !natural && rng.below(3) == 0 {
                Verdict::Fail
            } else {
                Verdict::Pass
            };
            let pass = result == Verdict::Pass;
            let outcome = Outcome {
                result,
                summary: "chaos".into(),
                data: None,
            };
            let times = 1 + rng.below(2);
            for _ in 0..times {
                let msg = Msg::Finish {
                    run: run.clone(),
                    token: token.clone(),
                    outcome: outcome.clone(),
                };
                let answer = w.core.send(None, msg).await.map(|r| r["answer"].clone());
                if pass && matches!(&answer, Ok(a) if a == "accepted") {
                    play.finished.insert(run.clone());
                }
            }
            play.log.push(format!("finish {run}#{attempt} pass={pass} x{times}"));
        }
        10..=15 => {
            // A decision on what a run waits for, once, twice or never (lost: its deadline decides).
            let waiting: Vec<_> = live.iter().filter_map(|r| Some((r.0.clone(), r.3?))).collect();
            let Some((run, seq)) = rng.pick(&waiting).cloned() else {
                return;
            };
            let journal = Journal::new(&w.journal).read_from(seq, 1).unwrap_or_default();
            let kind = journal.first().and_then(|e| e["type"].as_str()).unwrap_or_default();
            let decision = decision(rng, kind, natural);
            let times = if !natural && rng.below(6) == 0 {
                0
            } else {
                1 + rng.below(2)
            };
            for _ in 0..times {
                let msg = Msg::Decide {
                    run: run.clone(),
                    seq,
                    decision: decision.clone(),
                };
                let _ = w.core.send(None, msg).await;
            }
            play.log.push(format!("decide {run}@{seq} {decision:?} x{times}"));
        }
        16 => {
            let Some((run, ..)) = rng.pick(&live).cloned() else {
                return;
            };
            for _ in 0..1 + rng.below(2) {
                let msg = Msg::Cancel {
                    run: run.clone(),
                    reason: "chaos".into(),
                };
                let _ = w.core.send(None, msg).await;
            }
            play.log.push(format!("cancel {run}"));
            // The operator's cancel ends the run in its transaction, whatever automation had going (invariant 18).
            let state = runs(w).into_iter().find(|r| r.0 == run).unwrap().1;
            if !TERMINAL.contains(&state.as_str()) {
                play.broken = Some(format!("{run} is {state} after its cancel (invariant 18)"));
            }
        }
        17 => {
            let until_ms = (rng.below(2) == 0).then(|| w.clock.now_ms() + 30_000);
            let gate = GateSpec {
                key: "rig:shop/web".into(),
                until_ms,
                reason: "chaos".into(),
            };
            let _ = w.core.send(None, Msg::CloseGate(gate)).await;
            play.log.push(format!("close gate until {until_ms:?}"));
        }
        18 => {
            let msg = Msg::OpenGate {
                key: "rig:shop/web".into(),
            };
            let _ = w.core.send(None, msg).await;
            play.log.push("open gate".into());
        }
        19 => {
            // kill -9: the writer stops between two transactions, wherever the yields leave it.
            for _ in 0..rng.below(30) {
                tokio::task::yield_now().await;
            }
            w.restart().await;
            play.log.push("crash".into());
        }
        _ => {
            let ms = [10, 500, 5_000, 60_000, 600_000][usize::try_from(rng.below(5)).unwrap()];
            tokio::time::sleep(Duration::from_millis(ms)).await;
            play.log.push(format!("wait {ms} ms"));
        }
    }
}

/// The invariants that hold after every step.
fn check(w: &World, play: &Play) -> Result<(), String> {
    let journal = Journal::new(&w.journal)
        .read_from(1, 1_000_000)
        .map_err(|e| format!("{e:?}"))?;
    if let Some(broken) = &play.broken {
        return Err(broken.clone());
    }
    let mut last = 0;
    let mut ended: BTreeMap<String, String> = BTreeMap::new();
    for e in &journal {
        let seq = e["seq"].as_i64().unwrap();
        if seq <= last {
            return Err(format!("journal seq {seq} after {last}: an event delivered twice"));
        }
        last = seq;
        let (Some(run), Some(kind)) = (e["run"].as_str(), e["type"].as_str()) else {
            continue;
        };
        if let Some(end) = ended.get(run).filter(|_| kind.starts_with("run.")) {
            return Err(format!("{run}: {kind} at {seq} after {end} (an end is final)"));
        }
        if let Some(end) = kind.strip_prefix("run.").filter(|k| TERMINAL.contains(k)) {
            ended.insert(run.to_owned(), end.to_owned());
            if end == "passed" {
                if e["data"]["outcome"]["result"] != "pass" {
                    return Err(format!("{run} passed without a pass outcome: {}", e["data"]));
                }
                if !play.finished.contains(run) {
                    return Err(format!("{run} passed with no accepted pass finisher (invariant 12)"));
                }
            }
        }
    }
    let conn = state(w);
    let all = runs(w);
    if all.len() != play.runs.len() {
        return Err(format!(
            "{} runs for {} creates (invariant 10)",
            all.len(),
            play.runs.len()
        ));
    }
    for (run, state, attempt, _) in &all {
        if attempt > &20 {
            return Err(format!("{run}: attempt {attempt} over the ceiling (invariant 16)"));
        }
        let open: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE run = ?1 AND ended_ms IS NULL",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        if TERMINAL.contains(&state.as_str()) {
            if ended.get(run) != Some(state) {
                return Err(format!(
                    "{run} is {state} in the state, {:?} in the journal",
                    ended.get(run)
                ));
            }
            if open != 0 {
                return Err(format!("{run} is {state} with an attempt not ended"));
            }
            continue;
        }
        if open > 1 {
            return Err(format!("{run}: {open} attempts not ended (invariant 6)"));
        }
        let deadlines: i64 = conn
            .query_row("SELECT COUNT(*) FROM deadlines WHERE id LIKE ?1 || ':%'", [run], |r| {
                r.get(0)
            })
            .unwrap();
        if deadlines == 0 {
            return Err(format!("{run} is {state} with no deadline (invariant 14)"));
        }
    }
    Ok(())
}

/// Plays `seed`; the error is the failed check with the steps that led to it.
async fn play(seed: u64) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    let mut w = world(POLICY);
    let mut play = Play::default();
    let report = |play: &Play, why: String| format!("seed {seed}: {why}\nsteps:\n  {}", play.log.join("\n  "));
    for _ in 0..STEPS {
        step(&mut w, &mut rng, &mut play, false).await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        check(&w, &play).map_err(|why| report(&play, why))?;
    }
    // The faults stop. On even seeds nobody acts and the deadlines take every run to an end (invariant 14); on odd
    // ones the agent and the policy behave until every run ends.
    play.log.push("-- quiet --".into());
    for i in 0..300 {
        if runs(&w).iter().all(|r| TERMINAL.contains(&r.1.as_str())) {
            break;
        }
        // A run the operator's gate holds ends by its own deadline, after the calm steps.
        if seed % 2 == 1 && i < 100 {
            step(&mut w, &mut rng, &mut play, true).await;
            tokio::time::sleep(Duration::from_secs(20)).await;
        } else {
            tokio::time::sleep(Duration::from_secs(3_600)).await;
        }
        check(&w, &play).map_err(|why| report(&play, why))?;
    }
    let stuck: Vec<_> = runs(&w)
        .into_iter()
        .filter(|r| !TERMINAL.contains(&r.1.as_str()))
        .collect();
    if !stuck.is_empty() {
        return Err(report(&play, format!("no end after the faults stopped: {stuck:?}")));
    }
    Ok(())
}

fn seeds(from: u64) {
    let range = match std::env::var("AAE_CHAOS_SEED") {
        Ok(seed) => {
            let seed: u64 = seed.parse().unwrap();
            if !(from..from + SEEDS).contains(&seed) {
                return;
            }
            seed..seed + 1
        }
        Err(_) => from..from + SEEDS,
    };
    for seed in range {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        if let Err(why) = rt.block_on(play(seed)) {
            panic!("{why}");
        }
    }
}

#[test]
fn seeds_0() {
    seeds(0);
}

#[test]
fn seeds_1() {
    seeds(SEEDS);
}

#[test]
fn seeds_2() {
    seeds(2 * SEEDS);
}

#[test]
fn seeds_3() {
    seeds(3 * SEEDS);
}

#[test]
fn seeds_4() {
    seeds(4 * SEEDS);
}

#[test]
fn seeds_5() {
    seeds(5 * SEEDS);
}

#[test]
fn seeds_6() {
    seeds(6 * SEEDS);
}

#[test]
fn seeds_7() {
    seeds(7 * SEEDS);
}

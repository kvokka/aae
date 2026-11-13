//! The steps of the stand: what the clients, the harness, the policy, the operator and the faults do.

use crate::{
    Rng, TERMINAL,
    check::{runs, state},
    common::{World, hollow_turn},
};
use aae_core::{
    Journal,
    driver::{Normalized, SessionState, StopReason},
    run::{Create, Decision, GateSpec, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use std::{collections::BTreeMap, collections::BTreeSet, sync::Mutex, time::Duration};

/// Where the kills of this test landed, by what the state had recorded and the dead daemon had not done yet.
pub(crate) static KILLS: Mutex<BTreeMap<&str, u64>> = Mutex::new(BTreeMap::new());

/// What the state recorded and the daemon had not done yet: tracker calls, harness calls, hooks, journal lines.
fn undone(w: &World) -> Vec<&'static str> {
    let conn = state(w);
    let ops: Vec<String> = conn
        .prepare("SELECT DISTINCT json_extract(payload, '$.op') FROM effects")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let behind: bool = conn
        .query_row(
            "SELECT COALESCE((SELECT MAX(seq) FROM outbox), 0) > (SELECT seq FROM consumers WHERE name = 'journal')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut undone: BTreeSet<_> = ops
        .iter()
        .map(|op| match op.as_str() {
            "tracker" => "tracker",
            "hooks" => "hooks",
            "launch" | "prompt" | "respond" | "stop" | "stop_instance" => "harness",
            _ => "other",
        })
        .collect();
    if behind {
        undone.insert("journal");
    }
    undone.into_iter().collect()
}

/// What a test step did, for the replay of a failure.
#[derive(Default)]
pub(crate) struct Play {
    pub(crate) log: Vec<String>,
    /// Every run created, in order.
    pub(crate) runs: Vec<String>,
    /// The run each idempotency key of a create got.
    pub(crate) keyed: BTreeMap<u64, String>,
    /// The runs a `pass` finisher was accepted for.
    pub(crate) finished: BTreeSet<String>,
    /// Whether the last finisher accepted for each run said pass: the agent's word.
    pub(crate) said: BTreeMap<String, bool>,
    /// What a step saw break at once: a second run for a repeated key, a run still live after its cancel.
    pub(crate) broken: Option<String>,
    /// The journal as checked so far: the bytes read, the last `seq` and the end of every ended run.
    pub(crate) read: u64,
    pub(crate) seq: i64,
    pub(crate) ended: BTreeMap<String, String>,
}

/// A run of `build`, or of `fix` on `bead`.
fn create(bead: Option<&str>) -> Create {
    let inputs = bead.map(|b| ("bead".to_owned(), serde_json::json!(b)));
    Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: if bead.is_some() { "fix" } else { "build" }.into(),
        inputs: inputs.into_iter().collect(),
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: None,
    }
}

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
pub(crate) fn decision(rng: &mut Rng, kind: &str, natural: bool) -> Decision {
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
pub(crate) async fn step(w: &mut World, rng: &mut Rng, play: &mut Play, calm: bool) {
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
    // kill -9 cuts a step short that sets work going: a killer task among the writer's and the effects' dies after
    // a few turns of the scheduler, wherever the writer is then.
    let kill = matches!(op, 19 | 22);
    let op = if kill {
        *rng.pick(&[0, 7, 7, 10, 10, 16, 17]).unwrap()
    } else {
        op
    };
    let killer = kill.then(|| {
        let (writer, turns) = (w.killer(), rng.below(8));
        tokio::spawn(async move {
            // `yield_now` would wait until every other task is idle: a self-wake goes to the back of the queue.
            for _ in 0..turns {
                let mut woke = false;
                std::future::poll_fn(|cx| {
                    if std::mem::replace(&mut woke, true) {
                        return std::task::Poll::Ready(());
                    }
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                })
                .await;
            }
            writer.abort();
        })
    });
    act(w, rng, play, op, natural, &live).await;
    let Some(killer) = killer else {
        return;
    };
    killer.await.unwrap();
    let undone = undone(w);
    for at in &undone {
        *KILLS.lock().unwrap().entry(at).or_default() += 1;
    }
    w.restart().await;
    // Some crashes take the transcripts of the dead sessions with them: a resume must find out (invariant 17).
    let lost = rng.below(3) == 0;
    if lost {
        w.driver.lose_transcripts();
    }
    play.log
        .push(format!("crash before {undone:?} done, transcripts lost: {lost}"));
}

type Run = (String, String, u32, Option<i64>);

/// Step `op`, one of [`step`]'s.
async fn act(w: &mut World, rng: &mut Rng, play: &mut Play, op: u64, natural: bool, live: &[Run]) {
    match op {
        20 => {
            let down = rng.below(2) == 0;
            w.tracker.set_down(down);
            play.log.push(format!("tracker down={down}"));
        }
        0..=2 => {
            // A keyed create may come again: the client retried (invariant 10).
            // Keys 4 and 5 are runs on the beads b-1 and b-2: refused while another live run holds the bead.
            let key = rng.below(6);
            let bead = (key >= 4).then(|| format!("b-{}", key - 3));
            let msg = Msg::Create(create(bead.as_deref()));
            let run = match w.core.send(Some(&format!("create-{key}")), msg).await {
                Ok(reply) => reply["run"].as_str().unwrap().to_owned(),
                Err(e) => {
                    play.log.push(format!("create {key} refused: {e}"));
                    return;
                }
            };
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
            let pool = if running.is_empty() { live } else { &running };
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
                if matches!(&answer, Ok(a) if a == "accepted") {
                    play.said.insert(run.clone(), pass);
                    if pass {
                        play.finished.insert(run.clone());
                    }
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
            // The harness may drop the prompt a remark or a `continue` sends: only the watchdog moves on (invariant 15).
            let lost = !natural && rng.below(2) == 0;
            w.driver.lose_prompts(lost);
            for _ in 0..times {
                let msg = Msg::Decide {
                    run: run.clone(),
                    seq,
                    decision: decision.clone(),
                };
                let _ = w.core.send(None, msg).await;
            }
            w.driver.lose_prompts(false);
            play.log
                .push(format!("decide {run}@{seq} {decision:?} x{times}, prompt lost: {lost}"));
        }
        16 => {
            let Some((run, ..)) = rng.pick(live).cloned() else {
                return;
            };
            let mut answered = false;
            for _ in 0..1 + rng.below(2) {
                let msg = Msg::Cancel {
                    run: run.clone(),
                    reason: "chaos".into(),
                };
                answered |= w.core.send(None, msg).await.is_ok();
            }
            play.log.push(format!("cancel {run}"));
            // The operator's cancel ends the run in its transaction, whatever automation had going (invariant 18); a
            // kill may come first.
            let state = runs(w).into_iter().find(|r| r.0 == run).unwrap().1;
            if answered && !TERMINAL.contains(&state.as_str()) {
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
        _ => {
            let ms = [10, 500, 5_000, 60_000, 600_000][usize::try_from(rng.below(5)).unwrap()];
            tokio::time::sleep(Duration::from_millis(ms)).await;
            play.log.push(format!("wait {ms} ms"));
        }
    }
}

//! The instance registry (spec §7.3, docs/design/contracts.md 8, criterion 13 in the core): instances shared by
//! fingerprint up to their capacity, stopped once idle.

mod common;

use aae_core::{
    Journal, config,
    driver::{DriverEvent, Failure, FailureKind, InstanceHandle, Normalized, SessionRef, Source},
    run::{Create, Msg},
};
use aae_testkit::{ScriptedDriver, Step};
use common::{World, killed, world_driven};
use serde_json::{Value, json};
use std::{process::Child, sync::Arc, time::Duration};

/// Two rigs and two agents: `dev` and `same` share a process part, `other` has its own.
const FILES: &[(&str, &str)] = &[
    (
        "shop/aae.toml",
        "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
    ),
    ("shop/api/.keep", ""),
    ("shop/agents/same/agent.toml", "harness = \"scripted\"\n"),
    (
        "shop/agents/other/agent.toml",
        "harness = \"scripted\"\n[native.scripted]\naccount = \"b\"\n",
    ),
    (
        "shop/formulas/same.toml",
        "name = \"same\"\n[step]\nkind = \"prompt\"\nagent = \"same\"\nprompt = \"build.md\"\n",
    ),
    (
        "shop/formulas/other.toml",
        "name = \"other\"\n[step]\nkind = \"prompt\"\nagent = \"other\"\nprompt = \"build.md\"\n",
    ),
];

impl World {
    async fn start(&self, rig: &str, formula: &str) -> String {
        let create = Create {
            project: "shop".into(),
            rig: rig.into(),
            formula: formula.into(),
            inputs: serde_json::Map::new(),
            correlation: None,
            depth: 0,
            parent_run: None,
            continue_from: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// The `instance.*` events so far, `(type, instance id)`.
    fn instance_events(&self) -> Vec<(String, String)> {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        all.into_iter()
            .filter(|e| e["type"].as_str().unwrap().starts_with("instance."))
            .map(|e| (e["type"].as_str().unwrap().to_owned(), id(&e["data"])))
            .collect()
    }

    fn count(&self, kind: &str) -> usize {
        self.instance_events().iter().filter(|(t, _)| t == kind).count()
    }

    async fn cancel(&self, run: &str) {
        let msg = Msg::Cancel {
            run: run.into(),
            reason: "done".into(),
        };
        self.core.send(None, msg).await.unwrap();
    }

    /// A crash of session `session` reported on the first instance.
    async fn crash(&self, session: u64) {
        let event = DriverEvent {
            instance: self.instance_events()[0].1.clone(),
            session: Some(SessionRef(json!({ "session": session }))),
            seq: 1_000,
            event: Normalized::Failure(Failure::new(FailureKind::HarnessCrash, Source::Adapter)),
        };
        self.core.send(None, Msg::Driver(event)).await.unwrap();
    }
}

/// A real process marked with the first instance's id, as a tool command of the harness would be.
fn marked(w: &World) -> Child {
    common::marked(&w.instance_events()[0].1)
}

fn failed(events: &[(String, Value)]) -> bool {
    events.iter().any(|(t, _)| t == "run.attempt_failed")
}

fn id(data: &Value) -> String {
    data["instance"]["id"].as_str().unwrap().to_owned()
}

/// Two hanging sessions (no scenario: the prompt is acknowledged and the turn never ends).
async fn two_running(w: &World, second: &str) -> (String, String) {
    let one = w.start("web", "build").await;
    let two = w.start("api", second).await;
    for run in [&one, &two] {
        let events = w.events(run, Duration::from_secs(1)).await;
        assert!(
            events
                .iter()
                .any(|(t, d)| t == "run.state_changed" && d["to"] == "running")
        );
    }
    (one, two)
}

#[tokio::test(start_paused = true)]
async fn one_fingerprint_shares_an_instance_that_stops_after_its_idle_time() {
    let w = world_driven(FILES, ScriptedDriver::new(2).idle(Duration::from_secs(60)));
    let (one, two) = two_running(&w, "same").await;
    assert_eq!(w.count("instance.starting"), 1, "{:?}", w.instance_events());
    assert_eq!(w.count("instance.ready"), 1);
    w.cancel(&one).await;
    w.cancel(&two).await;
    tokio::time::sleep(Duration::from_secs(59)).await;
    assert_eq!(w.count("instance.stopped"), 0, "it idles for 60 s");
    // A new session in the idle time takes the same instance again.
    let three = w.start("web", "build").await;
    w.events(&three, Duration::from_secs(1)).await;
    assert_eq!(w.count("instance.starting"), 1);
    w.cancel(&three).await;
    tokio::time::sleep(Duration::from_secs(61)).await;
    assert_eq!(w.count("instance.stopped"), 1);
}

#[tokio::test(start_paused = true)]
async fn a_full_instance_or_another_fingerprint_gets_an_instance_of_its_own() {
    let w = world_driven(FILES, ScriptedDriver::new(1));
    two_running(&w, "same").await;
    assert_eq!(w.count("instance.starting"), 2, "one session per instance");
    let w = world_driven(FILES, ScriptedDriver::new(4));
    let (one, two) = two_running(&w, "other").await;
    assert_eq!(w.count("instance.starting"), 2, "two fingerprints");
    w.cancel(&one).await;
    w.cancel(&two).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.count("instance.stopped"), 2, "no idle time: stopped at once");
}

/// A reload stops the idle instances whose fingerprint no agent of the new config gives, and only those (spec §6.5);
/// a harness binary change is refused, since the drivers are made at start.
#[tokio::test(start_paused = true)]
async fn a_reload_stops_only_the_idle_instances_whose_fingerprint_changed() {
    let w = world_driven(FILES, ScriptedDriver::new(4).idle(Duration::from_secs(60)));
    let (one, two) = two_running(&w, "other").await;
    w.cancel(&one).await;
    w.cancel(&two).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let other = (w.instance_events().into_iter())
        .filter(|(t, _)| t == "instance.starting")
        .map(|(_, id)| id)
        .nth(1)
        .unwrap();
    let reload = |text: Option<&str>| {
        let path = w.dir.path().join("shop/agents/other/agent.toml");
        if let Some(text) = text {
            std::fs::write(&path, text).unwrap();
        }
        Arc::new(config::load(&w.dir.path().join("config.toml")).unwrap())
    };

    let same = w.core.send(None, Msg::Reload(reload(None))).await.unwrap();
    assert_eq!(same["restarted_instances"], json!([]), "nothing changed: {same}");
    let changed = reload(Some("harness = \"scripted\"\n[native.scripted]\naccount = \"c\"\n"));
    let reply = w.core.send(None, Msg::Reload(changed.clone())).await.unwrap();
    assert_eq!(reply["restarted_instances"], json!([other]), "{reply}");
    assert_eq!(w.count("instance.stopped"), 1, "{:?}", w.instance_events());

    let mut binary = (*changed).clone();
    binary.harnesses.get_mut("scripted").unwrap().binary = Some("/opt/other".into());
    let refused = w.core.send(None, Msg::Reload(Arc::new(binary))).await.unwrap_err();
    assert!(refused.to_string().contains("restart the daemon"), "{refused}");
    let mut sessions = (*changed).clone();
    let options = &mut sessions.harnesses.get_mut("scripted").unwrap().options;
    options.insert("max_sessions".to_owned(), 3.into());
    let refused = w.core.send(None, Msg::Reload(Arc::new(sessions))).await.unwrap_err();
    assert!(refused.to_string().contains("restart the daemon"), "{refused}");
    let after = w.core.send(None, Msg::Reload(changed)).await.unwrap();
    assert_eq!(
        after["config_generation"], 4,
        "the refused reload changed nothing: {after}"
    );
}

#[cfg(target_os = "linux")] // the sweep reads `/proc`
#[tokio::test(start_paused = true)]
async fn a_shared_instance_that_dies_fails_every_attempt_on_it() {
    let w = world_driven(FILES, ScriptedDriver::new(2));
    // The second prompt plays the death of the process.
    w.driver
        .script(vec![Step::Emit(Normalized::Activity { tool_running: false })]);
    w.driver.script(vec![
        Step::Wait(Duration::from_secs(5)),
        Step::Exit {
            code: Some(1),
            signal: None,
        },
    ]);
    let (one, two) = two_running(&w, "same").await;
    let orphan = marked(&w);
    for run in [&one, &two] {
        let events = w.events(run, Duration::from_secs(10)).await;
        let failed = common::last(&events, "run.attempt_failed");
        assert_eq!(failed["failure"]["kind"], "harness_crash", "{run}");
    }
    let failed = w.instance_events();
    assert_eq!(
        failed.iter().filter(|(t, _)| t == "instance.failed").count(),
        1,
        "{failed:?}"
    );
    assert!(killed(orphan).await, "its marked descendants are swept (invariant 7)");
}

/// A stop, not a death: the scripted exit lands on a `stopping` instance, so only the stop path sweeps (invariant 7).
#[cfg(target_os = "linux")] // the sweep reads `/proc`
#[tokio::test(start_paused = true)]
async fn a_stopped_instance_has_its_marked_descendants_swept() {
    let w = world_driven(FILES, ScriptedDriver::new(1));
    let run = w.start("web", "build").await;
    let events = w.events(&run, Duration::from_secs(1)).await;
    assert!(
        events
            .iter()
            .any(|(t, d)| t == "run.state_changed" && d["to"] == "running")
    );
    let orphan = marked(&w);
    w.cancel(&run).await;
    assert!(killed(orphan).await, "its marked descendants are swept");
    assert_eq!(w.count("instance.stopped"), 1, "{:?}", w.instance_events());
    assert_eq!(w.count("instance.failed"), 0);
}

#[tokio::test(start_paused = true)]
async fn a_late_event_of_another_session_leaves_the_live_attempt_alone() {
    let w = world_driven(FILES, ScriptedDriver::new(2));
    let (one, two) = two_running(&w, "same").await;
    w.cancel(&one).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    // The cancelled thread reports late; only `two` lives on the shared instance.
    w.crash(424_242).await;
    assert!(!failed(&w.events(&two, Duration::from_secs(1)).await));
}

#[tokio::test(start_paused = true)]
async fn an_event_before_the_session_is_known_reaches_the_only_attempt() {
    let w = world_driven(FILES, ScriptedDriver::new(1));
    w.driver.stall_sessions();
    let run = w.start("web", "build").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.crash(7).await;
    let events = w.events(&run, Duration::from_secs(1)).await;
    assert!(failed(&events), "{events:?}");
}

#[cfg(target_os = "linux")] // the sweep reads `/proc`
#[tokio::test(start_paused = true)]
async fn an_instance_found_dead_by_its_health_fails_every_attempt_on_it() {
    let w = world_driven(FILES, ScriptedDriver::new(2));
    w.driver.script(vec![]);
    w.driver.script(vec![Step::Vanish]);
    let (one, two) = two_running(&w, "same").await;
    let orphan = marked(&w);
    for run in [&one, &two] {
        let events = w.events(run, Duration::from_secs(31)).await;
        let failed = common::last(&events, "run.attempt_failed");
        assert_eq!(failed["failure"]["kind"], "harness_crash", "{run}");
    }
    assert_eq!(w.count("instance.failed"), 1);
    assert!(killed(orphan).await, "its marked descendants are swept");
}

/// The operator stops an idle instance only: one with a session or an unknown id is refused (contracts 8).
#[tokio::test(start_paused = true)]
async fn the_operator_stops_an_idle_instance_and_no_busy_one() {
    let w = world_driven(FILES, ScriptedDriver::new(2).idle(Duration::from_secs(600)));
    let run = w.start("web", "build").await;
    w.events(&run, Duration::from_secs(1)).await;
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let listed = reader.instances().unwrap();
    assert_eq!((listed.len(), &listed[0]["sessions"]), (1, &json!(1)), "{listed:?}");
    let id = listed[0]["id"].as_str().unwrap().to_owned();
    let stop = |id: &str| Msg::StopInstance { id: id.into() };

    let busy = w.core.send(None, stop(&id)).await.unwrap_err();
    assert!(busy.to_string().contains("1 session(s)"), "{busy}");
    let unknown = w.core.send(None, stop("nope")).await.unwrap_err();
    assert!(unknown.to_string().contains("no instance `nope`"), "{unknown}");

    w.cancel(&run).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.count("instance.stopped"), 0, "it idles for 600 s");
    w.core.send(None, stop(&id)).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    let stopped: Vec<_> = all.iter().filter(|e| e["type"] == "instance.stopped").collect();
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert_eq!(stopped[0]["data"]["reason"], "operator");
    assert!(reader.instances().unwrap().is_empty(), "the record goes");
}

/// A process gone with no exit reported is caught by the periodic resync (spec §5.1, invariant 20): the instance
/// fails as a crash and so does the attempt on it.
#[tokio::test(start_paused = true)]
async fn an_instance_whose_exit_was_lost_is_failed_by_the_resync() {
    let w = world_driven(FILES, ScriptedDriver::new(1));
    // Its health watcher looks at 40 s, 70 s, … 280 s, 310 s; the process vanishes at 295 s, the resync is at 300 s.
    w.driver
        .script(vec![Step::Wait(Duration::from_secs(285)), Step::Vanish]);
    tokio::time::sleep(Duration::from_secs(10)).await;
    let run = w.start("web", "build").await;
    let events = w.events(&run, Duration::from_secs(280)).await;
    assert!(!failed(&events), "alive until the resync");
    let events = w.events(&run, Duration::from_secs(15)).await;
    let failed = common::last(&events, "run.attempt_failed");
    assert_eq!(failed["failure"]["kind"], "harness_crash");
    // Failed at the resync, before the watcher's next look.
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    let failed = all.iter().find(|e| e["type"] == "instance.failed").unwrap();
    assert_eq!(failed["ts"], "1970-01-01T00:05:00.000Z", "{failed}");
    assert_eq!(w.count("instance.failed"), 1);
}

/// A resync report about a process the record no longer holds is stale and changes nothing.
#[tokio::test(start_paused = true)]
async fn a_resync_report_on_another_process_of_the_instance_is_ignored() {
    let w = world_driven(FILES, ScriptedDriver::new(1));
    let run = w.start("web", "build").await;
    w.events(&run, Duration::from_secs(1)).await;
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let listed = &reader.instances().unwrap()[0];
    let gone = |pid: &Value| Msg::InstanceGone {
        handle: InstanceHandle {
            id: listed["id"].as_str().unwrap().to_owned(),
            pid: i32::try_from(pid.as_i64().unwrap()).unwrap(),
            start_time: 0,
            pgid: i32::try_from(listed["pgid"].as_i64().unwrap()).unwrap(),
        },
    };
    w.core.send(None, gone(&json!(1))).await.unwrap();
    assert!(!failed(&w.events(&run, Duration::from_secs(1)).await));
    assert_eq!(w.count("instance.failed"), 0);
    w.core.send(None, gone(&listed["pid"])).await.unwrap();
    assert!(failed(&w.events(&run, Duration::from_secs(1)).await));
    assert_eq!(w.count("instance.failed"), 1);
}

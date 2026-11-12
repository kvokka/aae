//! The instance registry (spec §7.3, docs/design/contracts.md 8, criterion 13 in the core): instances shared by
//! fingerprint up to their capacity, stopped once idle.

mod common;

use aae_core::{
    Journal,
    driver::Normalized,
    run::{Create, Msg},
};
use aae_testkit::{ScriptedDriver, Step};
use common::{World, world_driven};
use serde_json::Value;
use std::time::Duration;

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

#[tokio::test(start_paused = true)]
async fn a_shared_instance_that_dies_fails_every_attempt_on_it() {
    let w = world_driven(FILES, ScriptedDriver::new(2));
    // The second prompt plays the death of the process.
    w.driver
        .script(vec![Step::Emit(Normalized::Activity { tool_running: false })]);
    w.driver.script(vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]);
    let (one, two) = two_running(&w, "same").await;
    for run in [&one, &two] {
        let events = w.events(run, Duration::from_secs(1)).await;
        let failed = common::last(&events, "run.attempt_failed");
        assert_eq!(failed["failure"]["kind"], "harness_crash", "{run}");
    }
    let failed = w.instance_events();
    assert_eq!(
        failed.iter().filter(|(t, _)| t == "instance.failed").count(),
        1,
        "{failed:?}"
    );
}

//! Helpers of the bead tests: a formula on one bead, its runs and the bead events in the journal.

use super::{World, hollow_turn};
use aae_core::run::{Create, Msg};
use aae_testkit::Step;
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) const FIX: &[(&str, &str)] = &[
    (
        "shop/formulas/fix.toml",
        "name = \"fix\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
         [[step]]\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"fix.md\"\n",
    ),
    ("shop/formulas/fix.md", "Fix {{ bead.title }}."),
];

impl World {
    pub(crate) async fn fix(&self, bead: Value) -> Result<Value, aae_core::Error> {
        self.fix_on(Some("web"), bead).await
    }

    /// A start of `fix` on `bead` on `rig`; `None` names no rig.
    pub(crate) async fn fix_on(&self, rig: Option<&str>, bead: Value) -> Result<Value, aae_core::Error> {
        self.start_bead("fix", rig, bead).await
    }

    /// A start of `formula` with the input `bead` on `rig`.
    pub(crate) async fn start_bead(
        &self,
        formula: &str,
        rig: Option<&str>,
        bead: Value,
    ) -> Result<Value, aae_core::Error> {
        let create = Create {
            project: "shop".into(),
            rig: rig.map(Into::into),
            formula: formula.into(),
            inputs: [("bead".to_owned(), bead)].into_iter().collect(),
            correlation: None,
            depth: 0,
            parent_run: None,
            token: None,
            continue_from: None,
        };
        self.core.send(None, Msg::Create(create)).await
    }

    /// A run of `fix` on `bead`, by its id.
    pub(crate) async fn run_on(&self, bead: Value) -> String {
        let reply = self.fix(bead).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    pub(crate) fn assignee(&self, bead: &str) -> Option<String> {
        self.tracker.bead(bead).unwrap().assignee
    }

    /// The tracker says it changed; the core reads its ready set again.
    pub(crate) async fn changed(&self) {
        let msg = Msg::TrackerChanged("shop".into());
        self.core.send(None, msg).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) fn next_ready() -> Value {
    json!({ "next_ready": true })
}

/// A turn that ends 10 s after the prompt: time for the finisher.
pub(crate) fn slow_turn() -> Vec<Step> {
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(10)));
    steps
}

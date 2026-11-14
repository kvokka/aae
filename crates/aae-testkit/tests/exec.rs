//! The exec step (spec §9.1, docs/design/contracts.md 6.2): real commands, so real time.

mod common;

use aae_core::{
    Journal,
    run::{Create, ExecHooks, Msg},
};
use common::{World, world_with};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// A world with the exec formula `check` running `sh -c <script>` with a 2 s timeout.
fn check(script: &str) -> (World, Create) {
    check_with("", script)
}

/// [`check`] with the `[inputs]` table `inputs`.
fn check_with(inputs: &str, script: &str) -> (World, Create) {
    let formula = format!(
        "name = \"check\"\n[inputs]\n{inputs}[step]\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {}]\n\
         timeout = \"2s\"\n",
        json!(script)
    );
    let w = world_with(
        &[("shop/formulas/check.toml", &formula)],
        Arc::new(ExecHooks::default()),
    );
    let create = Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: "check".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: None,
    };
    (w, create)
}

impl World {
    async fn start(&self, create: Create) -> String {
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// The first event of `run` among `kinds`, waiting for it up to 10 s.
    async fn first(&self, run: &str, kinds: &[&str]) -> (String, Value) {
        let started = Instant::now();
        loop {
            let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap_or_default();
            let found = all
                .into_iter()
                .find(|e| e["run"] == run && kinds.iter().any(|k| e["type"] == *k));
            if let Some(e) = found {
                return (e["type"].as_str().unwrap().to_owned(), e["data"].clone());
            }
            assert!(started.elapsed() < Duration::from_secs(10), "none of {kinds:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn end(&self, run: &str) -> (String, Value) {
        self.first(run, &["run.passed", "run.failed", "run.cancelled"]).await
    }
}

#[tokio::test]
async fn exit_0_passes_with_the_output_tail_in_the_outcome() {
    let (w, create) = check("echo \"$TRACKER_ACTOR\"; pwd; echo warn >&2");
    let run = w.start(create).await;
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed");
    assert_eq!(
        (&data["run"]["step_kind"], &data["run"]["agent"]),
        (&json!("exec"), &Value::Null)
    );
    let out = &data["outcome"]["data"];
    assert_eq!((&out["exit_code"], &out["stderr"]), (&json!(0), &json!("warn\n")));
    let stdout = out["stdout"].as_str().unwrap();
    assert!(stdout.ends_with("/shop/web\n"), "runs in the rig: {out}");
    assert!(
        stdout.starts_with(&format!("aae:{run}:1\n")),
        "the attempt is the tracker actor: {out}"
    );
}

#[tokio::test]
async fn the_command_gets_the_run_and_its_inputs_in_the_environment_and_inputs_are_typed() {
    let inputs = "n = { type = \"integer\" }\nflag = { type = \"boolean\" }\nnote-text = { type = \"string\" }\n";
    let (w, _) = check_with(
        inputs,
        "echo \"$AAE_RUN $AAE_INPUT_N $AAE_INPUT_FLAG $AAE_INPUT_NOTE_TEXT ${AAE_BEAD-none}\"",
    );
    let with = |inputs: Value| Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: "check".into(),
        inputs: inputs.as_object().unwrap().clone(),
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: None,
    };
    let run = w
        .start(with(json!({ "n": 3, "flag": true, "note-text": "a {b}" })))
        .await;
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let stdout = &data["outcome"]["data"]["stdout"];
    assert_eq!(stdout, &json!(format!("{run} 3 true a {{b}} none\n")));
    for (input, value) in [("n", json!("3")), ("flag", json!(1))] {
        let refused = w
            .core
            .send(None, Msg::Create(with(json!({ input: value }))))
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("must be a"), "{input}: {refused}");
    }
}

#[tokio::test]
async fn another_code_fails_the_run() {
    let (w, create) = check("exit 3");
    let run = w.start(create).await;
    let (kind, data) = w.end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"], &data["reason"]),
        ("run.failed", &json!("agent"), &json!("exit 3"))
    );
}

#[tokio::test]
async fn exit_75_is_an_attempt_failure_not_an_outcome() {
    let (w, create) = check("exit 75");
    let run = w.start(create).await;
    let (_, failed) = w.first(&run, &["run.attempt_failed"]).await;
    assert_eq!(failed["failure"]["text"], "exit 75 (EX_TEMPFAIL)");
    let (kind, data) = w.end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("infrastructure"))
    );
}

#[tokio::test]
async fn a_command_past_its_timeout_is_a_runaway() {
    let (w, create) = check("sleep 30");
    let run = w.start(create).await;
    let (_, failed) = w.first(&run, &["run.attempt_failed"]).await;
    assert_eq!(failed["failure"]["kind"], "runaway_turn");
}

#[tokio::test]
async fn cancelling_the_run_stops_the_command() {
    let (w, create) = check("echo $$ > pid; exec sleep 30");
    let pid_file = w.journal.parent().unwrap().join("shop/web/pid");
    let run = w.start(create).await;
    let started = Instant::now();
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file).ok().filter(|p| p.ends_with('\n')) {
            break pid.trim().to_owned();
        }
        assert!(started.elapsed() < Duration::from_secs(5), "the command did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "enough".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    assert_eq!(w.end(&run).await.0, "run.cancelled");
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", &pid])
            .status()
            .is_ok_and(|s| s.success())
    };
    while alive() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the command outlived the run"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

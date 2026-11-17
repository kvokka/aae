//! The environment of an exec step: `[exec]` of the orchestrator and the project, the step's `inherit_env` and `env`,
//! `${NAME}` references to the daemon's variables (docs/design/config.md, "Environment values"). Real commands, so
//! real time. The test process has `CARGO_PKG_NAME`, set by cargo, and no `AAE_TEST_NO_SUCH_VARIABLE`.

use crate::common;

use aae_core::run::{Create, ExecHooks, Msg};
use common::{World, world_with};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const ORCHESTRATOR: &str =
    "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n";

/// A world whose formula `check` runs `sh -c <script>` with the step keys `step`, the orchestrator's `[exec]` being
/// `top` and the project's `own`.
fn world_of(top: &str, own: &str, step: &str, script: &str) -> World {
    world_hooked(top, own, step, script, "", &[])
}

/// [`world_of`] with `plugins` in the orchestrator's config and the files `extra`; hooks run as commands.
fn world_hooked(top: &str, own: &str, step: &str, script: &str, plugins: &str, extra: &[(&str, &str)]) -> World {
    let config = format!("{ORCHESTRATOR}{plugins}[exec]\n{top}\n");
    let project = format!("[rigs.web]\npath = \"web\"\n[exec]\n{own}\n");
    let formula = format!(
        "name = \"check\"\n[[step]]\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {}]\ntimeout = \"5s\"\n{step}\n",
        json!(script)
    );
    let mut files = vec![
        ("config.toml", config.as_str()),
        ("shop/aae.toml", project.as_str()),
        ("shop/formulas/check.toml", formula.as_str()),
    ];
    files.extend_from_slice(extra);
    world_with(&files, Arc::new(ExecHooks::default()))
}

impl World {
    async fn start_check(&self) -> String {
        let create = Create {
            project: "shop".into(),
            rig: Some("web".into()),
            formula: "check".into(),
            inputs: serde_json::Map::new(),
            correlation: None,
            depth: 0,
            parent_run: None,
            token: None,
            continue_from: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// The stdout of the command of a run of `check` that passed.
    async fn stdout(&self) -> (String, String) {
        let run = self.start_check().await;
        let (kind, data) = self.repeat_end(&run).await;
        assert_eq!(kind, "run.passed", "{data}");
        let out = data["outcome"]["data"]["stdout"].as_str().unwrap().to_owned();
        (run, out)
    }
}

#[tokio::test]
async fn a_step_gets_literals_references_and_a_dollar() {
    let top =
        r#"env = { LIT = "plain", GH_TOKEN = "${CARGO_PKG_NAME}", PRICE = "$$5", BOTH = "$${X}${CARGO_PKG_NAME}" }"#;
    let w = world_of(
        top,
        "",
        "",
        r#"printf '%s %s %s %s' "$LIT" "$GH_TOKEN" "$PRICE" "$BOTH""#,
    );
    assert_eq!(w.stdout().await.1, "plain aae-testkit $5 ${X}aae-testkit");
}

#[tokio::test]
async fn the_environment_is_clean_unless_inherit_env() {
    let script = r#"printf '%s %s' "${CARGO_PKG_NAME-unset}" "${PATH:+path}""#;
    let w = world_of("", "", "", script);
    assert_eq!(w.stdout().await.1, "unset path", "only PATH, HOME and LANG");
    let w = world_of("", "", "inherit_env = true", script);
    assert_eq!(w.stdout().await.1, "aae-testkit path");
    // `env` is over what is inherited, and the variables of aae over both.
    let script = r#"printf '%s %s' "$CARGO_PKG_NAME" "$AAE_RUN""#;
    let step = "inherit_env = true\nenv = { CARGO_PKG_NAME = \"mine\", AAE_RUN = \"mine\" }";
    let w = world_of("", "", step, script);
    let (run, out) = w.stdout().await;
    assert_eq!(out, format!("mine {run}"));
}

#[tokio::test]
async fn the_step_is_over_the_project_over_the_orchestrator() {
    let top = "inherit_env = true\nenv = { A = \"top\", B = \"top\", C = \"top\" }";
    let own = "inherit_env = false\nenv = { B = \"own\", C = \"own\" }";
    let script = r#"printf '%s %s %s %s' "$A" "$B" "$C" "${CARGO_PKG_NAME-unset}""#;
    let w = world_of(top, own, "env = { C = \"step\" }", script);
    assert_eq!(w.stdout().await.1, "top own step unset");
    // The scalar is the strongest layer's that says it.
    let w = world_of(top, "", "", script);
    assert_eq!(w.stdout().await.1, "top top top aae-testkit");
}

#[tokio::test]
async fn a_variable_the_daemon_lacks_fails_the_step_as_config_error() {
    let step = "env = { GH_TOKEN = \"tok-${AAE_TEST_NO_SUCH_VARIABLE}-en\" }";
    let w = world_of("", "", step, "touch ran");
    let run = w.start_check().await;
    let failed = w.until(Some(&run), "run.attempt_failed", 0).await;
    let failure = &failed["failure"];
    assert_eq!(failure["kind"], "config_error", "{failed}");
    let text = failure["text"].as_str().unwrap();
    assert!(
        text.contains("`GH_TOKEN`") && text.contains("`AAE_TEST_NO_SUCH_VARIABLE`"),
        "{text}"
    );
    assert!(!text.contains("tok-"), "the value is not quoted: {text}");
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("infrastructure")),
        "{data}"
    );
    assert!(!w.dir.path().join("shop/web/ran").exists(), "the command did not run");
}

#[tokio::test]
async fn the_variables_of_aae_win_over_env() {
    let top = r#"env = { AAE_RUN = "mine", TRACKER_ACTOR = "mine" }"#;
    let w = world_of(top, "", "", r#"printf '%s %s' "$AAE_RUN" "$TRACKER_ACTOR""#);
    let (run, out) = w.stdout().await;
    assert_eq!(out, format!("{run} aae:{run}:1"));
}

/// Waits up to 10 s for `found`.
async fn wait_for<T>(mut found: impl FnMut() -> Option<T>) -> T {
    let started = Instant::now();
    loop {
        if let Some(value) = found() {
            return value;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "it did not come");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn the_snapshot_and_the_effect_keep_the_reference() {
    // The command waits for the test to read its effect, which is gone once it ends.
    let script = r#"until [ -e go ]; do sleep 0.02; done; printf %s "$GH_TOKEN""#;
    let w = world_of("", "", "env = { GH_TOKEN = \"${CARGO_PKG_NAME}\" }", script);
    let run = w.start_check().await;
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let query = |sql: &str| db.query_row(sql, [&run], |r| r.get::<_, String>(0)).ok();
    let effect = wait_for(|| query("SELECT payload FROM effects WHERE key = 'exec:' || ?1")).await;
    let effect: Value = serde_json::from_str(&effect).unwrap();
    assert_eq!(effect["env"]["GH_TOKEN"], "${CARGO_PKG_NAME}", "{effect}");
    let config = query("SELECT config FROM runs WHERE id = ?1").unwrap();
    let config: Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["env"]["GH_TOKEN"]["value"], "${CARGO_PKG_NAME}", "{config}");
    std::fs::write(w.dir.path().join("shop/web/go"), "").unwrap();
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(data["outcome"]["data"]["stdout"], "aae-testkit");
}

#[tokio::test]
async fn a_dollar_from_a_hook_reaches_the_command_as_it_is() {
    let plugins = "[settings]\nplugins = [{ name = \"policy\" }]\n[[plugin]]\npath = \"policy\"\n";
    let manifest = "name = \"policy\"\nversion = \"0.1.0\"\n[[hook]]\non = \"run.preparing\"\n\
        command = [\"sh\", \"prepare.sh\"]\ntimeout = \"5s\"\n";
    let prepare = r#"echo '{"decision":"proceed","env":{"X":"a$b${Y}"}}'"#;
    let extra = [("policy/plugin.toml", manifest), ("policy/prepare.sh", prepare)];
    let w = world_hooked("", "", "", r#"printf %s "$X""#, plugins, &extra);
    assert_eq!(w.stdout().await.1, "a$b${Y}");
}

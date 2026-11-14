//! The demos as the daemon loads them: the user's layers decide what the pack's agents run on, and the role prompt of
//! an agent (`agents/<a>/prompt.md`, spec §8.1) reaches what both adapters start a session with.

use aae_core::config::{Config, EffectiveConfig, Layer, load_for};
use aae_core::driver::{Adapter, Driver, SessionPlan};
use std::path::{Path, PathBuf};

const ADAPTERS: &[&Adapter] = &[&aae_claude::ADAPTER, &aae_codex::ADAPTER];

fn demos() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos")
}

fn demo(name: &str) -> Config {
    load_for(&demos().join(name).join("root/config.toml"), ADAPTERS).unwrap()
}

#[test]
fn the_user_layers_of_the_demos_decide_what_the_workers_run_on() {
    let config = demo("snake-claude");
    for (formula, rig) in [("dev.plan", "root"), ("dev.code", "backend"), ("dev.code", "frontend")] {
        let (_, effective) = config.for_run("snake-claude", rig, formula).unwrap();
        let model = effective.model.unwrap();
        assert_eq!((effective.harness.as_str(), model.value.as_str()), ("claude", "haiku"));
        assert_eq!(model.from, Layer::ProjectOverride, "{formula} on {rig}");
    }

    let config = demo("snake-mixed");
    let (agent, effective) = config.for_run("snake-mixed", "backend", "dev.code").unwrap();
    assert_eq!(agent, "codex-coder");
    assert_eq!(
        (effective.harness.as_str(), effective.harness_from),
        ("codex", Layer::Derived)
    );
    let model = effective.model.unwrap();
    assert_eq!(model.value, "gpt-6-luna");
    assert_eq!(effective.model_given.as_deref(), Some("luna"));
    let (agent, effective) = config.for_run("snake-mixed", "frontend", "dev.code").unwrap();
    assert_eq!((agent.as_str(), effective.harness.as_str()), ("dev.coder", "claude"));
}

#[test]
fn the_role_prompt_of_the_demo_agent_reaches_both_adapters() {
    let config = demo("snake-mixed");
    let (_, mut effective) = config.for_run("snake-mixed", "frontend", "dev.code").unwrap();
    let role = std::fs::read_to_string(demos().join("dev-pack/agents/coder/prompt.md")).unwrap();
    let prompt = effective.system_prompt.clone().unwrap();
    assert_eq!((prompt.value.as_str(), prompt.from), (role.as_str(), Layer::Agent));
    assert!(effective.explain().contains("system_prompt = \"You write code."));

    let state = tempfile::tempdir().unwrap();
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: None,
    };
    let delivered = |driver: &dyn Driver, config: &EffectiveConfig| {
        let split = driver.split(config, &plan).unwrap();
        format!("{} {}", split.process.0, split.session.0)
    };
    let needle = "you test what you wrote";
    let claude = aae_claude::Claude::new("claude".into(), state.path().into(), None);
    assert!(delivered(&claude, &effective).contains(needle));
    effective.harness = "codex".into();
    let codex = aae_codex::Codex::new("codex".into(), state.path().into(), None, 1);
    assert!(delivered(&codex, &effective).contains(needle));
}

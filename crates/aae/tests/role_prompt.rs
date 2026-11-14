//! The role prompt of an agent (`agents/<a>/prompt.md`, spec §8.1) reaches what both adapters start a session with.

use aae_core::config::{EffectiveConfig, Layer, Name, load};
use aae_core::driver::{Driver, SessionPlan};
use std::path::Path;

#[test]
fn the_role_prompt_of_the_demo_agent_reaches_both_adapters() {
    let demos = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos");
    let config = load(&demos.join("config.toml")).unwrap();
    let agent = Name {
        pack: "snake".into(),
        name: "frontend".into(),
    };
    let mut effective = config.effective("snake-mixed", Some("frontend"), &agent).unwrap();
    let role = std::fs::read_to_string(demos.join("snake/agents/frontend/prompt.md")).unwrap();
    let prompt = effective.system_prompt.clone().unwrap();
    assert_eq!((prompt.value.as_str(), prompt.from), (role.as_str(), Layer::Agent));
    assert!(effective.explain().contains("system_prompt = \"You write small"));

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
    let needle = "dependency-free web pages";
    let claude = aae_claude::Claude::new("claude".into(), state.path().into(), None);
    assert!(delivered(&claude, &effective).contains(needle));
    effective.harness = "codex".into();
    let codex = aae_codex::Codex::new("codex".into(), state.path().into(), None, 1);
    assert!(delivered(&codex, &effective).contains(needle));
}

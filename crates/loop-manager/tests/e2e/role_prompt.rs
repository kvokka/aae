//! Demo 1 as the daemon loads it: the user's layers decide what the pack's agents run on, and the role prompt of an
//! agent (`agents/<a>/prompt.md`, docs/design/config.md) reaches what both adapters start a session with.

use loop_manager_core::config::{Config, EffectiveConfig, Layer, load_for};
use loop_manager_core::driver::{Adapter, Driver, SessionPlan};
use std::path::Path;

const ADAPTERS: &[&Adapter] = &[&loop_manager_claude::ADAPTER, &loop_manager_codex::ADAPTER];

const DEMO: &str = "1-planner-executor";
const PROJECT: &str = "planner-executor";

fn load(demo: &Path) -> Config {
    load_for(&demo.join("root/config.toml"), ADAPTERS, Default::default()).unwrap()
}

/// The pack of the demo, its project files and its empty rigs into `to`: not what a run of it left behind.
fn copy_demo(to: &Path) {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos").join(DEMO);
    copy(&demo.join("pack"), &to.join("pack"));
    for dir in ["root", "frontend", "backend"] {
        std::fs::create_dir_all(to.join(dir)).unwrap();
    }
    for file in ["root/config.toml", "root/loop-manager.toml"] {
        std::fs::copy(demo.join(file), to.join(file)).unwrap();
    }
}

/// `from` into `to`, recursively.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy(&path, &target)
        } else {
            std::fs::copy(&path, &target).map(drop).unwrap()
        }
    }
}

#[test]
fn the_user_layers_of_the_demo_decide_what_the_agents_run_on() {
    let config = load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos").join(DEMO));
    let runs = [("dev.deliver", "plan", "root"), ("dev.implement", "code", "frontend")];
    for (formula, step, rig) in runs {
        let (_, effective) = config.for_run(PROJECT, rig, formula, step).unwrap();
        let model = effective.model.unwrap();
        assert_eq!((effective.harness.as_str(), model.value.as_str()), ("claude", "haiku"));
        assert_eq!(model.from, Layer::ProjectAgent, "{formula} on {rig}");
    }
    let (agent, effective) = config.for_run(PROJECT, "backend", "dev.implement", "code").unwrap();
    assert_eq!(agent, "dev.coder");
    assert_eq!(
        (effective.harness.as_str(), effective.harness_from),
        ("codex", Layer::RigAgent)
    );
    assert_eq!(effective.model.unwrap().value, "gpt-6-luna");
    assert_eq!(effective.model_given.as_deref(), Some("luna"));
}

#[test]
fn the_role_prompt_of_a_pack_agent_reaches_both_adapters() {
    let demo = tempfile::tempdir().unwrap();
    copy_demo(demo.path());
    let role = "You write code. Before you say it works, you test what you wrote.";
    std::fs::write(demo.path().join("pack/agents/coder/prompt.md"), role).unwrap();
    let config = load(demo.path());
    let (_, mut effective) = config.for_run(PROJECT, "frontend", "dev.implement", "code").unwrap();
    let prompt = effective.system_prompt.clone().unwrap();
    assert_eq!((prompt.value.as_str(), prompt.from), (role, Layer::Agent));
    assert!(effective.explain().contains("system_prompt = \"You write code."));

    let state = tempfile::tempdir().unwrap();
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: None,
        bead: None,
    };
    let delivered = |driver: &dyn Driver, config: &EffectiveConfig| {
        let split = driver.split(config, &plan).unwrap();
        format!("{} {}", split.process.0, split.session.0)
    };
    let needle = "you test what you wrote";
    let claude = loop_manager_claude::Claude::new("claude".into(), state.path().into(), None);
    assert!(delivered(&claude, &effective).contains(needle));
    effective.harness = "codex".into();
    let codex = loop_manager_codex::Codex::new("codex".into(), state.path().into(), None, 1);
    assert!(delivered(&codex, &effective).contains(needle));
}

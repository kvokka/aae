//! The `[builtin.<name>]` tables of the orchestrator config (spec §11.5).

use super::config::{fails, load_with};
use crate::config::load;

#[test]
fn no_orchestrator_file_is_an_empty_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = load(&dir.path().join("config.toml")).unwrap();
    assert!(config.projects.is_empty() && config.harnesses.is_empty());
    let on: Vec<_> = config.bundled.iter().map(|b| b.name).collect();
    assert_eq!(on, ["default-policy"], "worktree acts only where it is turned on");
}

#[test]
fn builtin_tables_turn_bundled_plugins_on_and_off() {
    let config = |text: &str| load_with(&[("config.toml", text)]);
    let on = config("[builtin.default-policy]\nenabled = false\n[builtin.worktree]\nagents = [\"dev\"]\n").unwrap();
    let on: Vec<_> = on.bundled.iter().map(|b| (b.name, b.params.to_string())).collect();
    assert_eq!(on, [("worktree", r#"{"agents":["dev"]}"#.to_owned())]);
    fails(
        &[("config.toml", "[builtin.policy]\n")],
        "`policy` is not a bundled plugin",
    );
    fails(
        &[("config.toml", "[builtin.worktree]\nenabled = 1\n")],
        "`enabled` is a boolean",
    );
}

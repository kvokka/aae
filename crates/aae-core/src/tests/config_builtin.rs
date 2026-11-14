//! The `[builtin.<name>]` tables of the orchestrator config and the formula key `worktree` (spec §11.5).

use super::config::{BASE, fails, load_with};
use crate::config::{Bundled, load};

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
    let off = config("[builtin.default-policy]\nenabled = false\n").unwrap();
    assert!(off.bundled.is_empty());
    fails(
        &[("config.toml", "[builtin.policy]\n")],
        "`policy` is not a bundled plugin",
    );
    fails(
        &[("config.toml", "[builtin.default-policy]\nenabled = 1\n")],
        "`enabled` is a boolean",
    );
}

#[test]
fn worktree_is_on_for_the_formulas_that_ask_for_it() {
    let names = |b: &[Bundled]| b.iter().map(|b| (b.name, b.params.to_string())).collect::<Vec<_>>();
    let config = load_with(&[]).unwrap();
    assert_eq!(
        names(&config.projects[0].bundled),
        [("default-policy", "{}".to_owned())]
    );
    let audit = "name = \"audit\"\nworktree = true\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n";
    let config = load_with(&[("kit/formulas/audit.toml", audit)]).unwrap();
    assert!(config.projects[0].packs[1].formulas["audit"].worktree);
    assert_eq!(
        names(&config.projects[0].bundled),
        [
            ("default-policy", "{}".to_owned()),
            ("worktree", r#"{"formulas":["kit.audit"]}"#.to_owned())
        ]
    );
    assert!(
        config.bundled.iter().all(|b| b.name != "worktree"),
        "no formula outside a project"
    );
    let wrong = audit.replace("worktree = true", "worktree = \"yes\"");
    fails(&[("kit/formulas/audit.toml", wrong.as_str())], "worktree");
}

#[test]
fn the_old_worktree_table_names_its_replacement() {
    let replacement = "set `worktree = true` in the formulas whose runs need a worktree";
    fails(
        &[("config.toml", "[builtin.worktree]\nagents = [\"dev\"]\n")],
        replacement,
    );
    let project = format!("{}[builtin.worktree]\nformulas = [\"audit\"]\n", BASE[3].1);
    fails(&[("shop/aae.toml", project.as_str())], replacement);
}

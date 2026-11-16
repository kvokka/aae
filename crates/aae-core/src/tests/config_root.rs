//! The project root: its name and path, and the rigs beside it (spec §6.1, decision 99).

use super::{
    config::{BASE, fails, load_for_adapters, load_with},
    config_launch::{ADAPTERS, HARNESSES},
};
use crate::config::{Name, Scope};

/// `shop/aae.toml` of `BASE` with `extra` before its rigs.
fn project(extra: &str) -> String {
    BASE[3]
        .1
        .replacen("[rigs.backend]", &format!("{extra}[rigs.backend]"), 1)
}

#[test]
fn a_project_without_a_root_section_has_the_root_next_to_its_file() {
    let config = load_with(&[]).unwrap();
    let shop = &config.projects[0];
    assert_eq!(shop.root.0, "root");
    assert_eq!(shop.root.1.path, shop.dir);
    assert_eq!(shop.place("root").unwrap().path, shop.dir);
    assert_eq!(shop.places().map(|(n, _)| n).collect::<Vec<_>>(), ["root", "backend"]);
}

#[test]
fn the_root_takes_a_name_and_a_path() {
    let text = project("[root]\nname = \"mono\"\npath = \"../work\"\nvars = { test_cmd = \"make\" }\n");
    let config = load_with(&[("shop/aae.toml", &text), ("work/.keep", "")]).unwrap();
    let shop = &config.projects[0];
    assert_eq!(shop.root.0, "mono");
    assert_eq!(shop.root.1.path, shop.dir.parent().unwrap().join("work"));
    assert_eq!(shop.root.1.vars["test_cmd"], "make");
    assert!(shop.dir.ends_with("shop"), "the config stays where aae.toml is");
    assert!(shop.place("root").is_none());
    // The root takes the rules of a rig's name and path.
    fails(
        &[("shop/aae.toml", &project("[root]\npath = \"../nowhere\"\n"))],
        "root `root`: ",
    );
    fails(
        &[("shop/aae.toml", &project("[root]\nname = \"a b\"\n"))],
        "root name `a b`",
    );
    fails(
        &[("shop/aae.toml", &project("[root]\nmodel = \"opus\"\n"))],
        "unknown field `model`",
    );
}

#[test]
fn a_rig_at_the_root_path_or_under_its_name_is_an_error() {
    let at_root = project("[rigs.top]\npath = \".\"\n");
    fails(&[("shop/aae.toml", &at_root)], "rig `top`: ");
    fails(&[("shop/aae.toml", &at_root)], "is the project root: drop the rig");
    let named_root = project("[root]\nname = \"backend\"\n");
    fails(
        &[("shop/aae.toml", &named_root)],
        "rig `backend`: the root has this name: rename the rig or set `[root] name`",
    );
}

#[test]
fn a_project_of_one_repository_checks_its_formulas_on_the_root() {
    // No rigs: the runs of the project's formulas still assemble, in the root, and their harness is the build's.
    let text = "packs = [\"../kit\"]\n[agents.implementer]\nharness = \"codex\"\n";
    let config = load_for_adapters(&[HARNESSES, ("shop/aae.toml", text)], ADAPTERS).unwrap();
    assert!(config.projects[0].rigs.is_empty());
    assert!(config.harnesses.contains_key("codex"));
    let (agent, _) = config.for_run("shop", "root", "kit.implement", "1").unwrap();
    assert_eq!(agent, "kit.implementer");
}

#[test]
fn an_agent_of_scope_root_takes_no_rig_layer() {
    let agent = "kit/agents/implementer/agent.toml";
    let rig_wide = BASE[3]
        .1
        .replace("[rigs.backend.agents.implementer]", "[rigs.backend.settings]");
    let rig_agent = load_with(&[("shop/aae.toml", &rig_wide)]).unwrap();
    let (_, e) = rig_agent.for_run("shop", "backend", "kit.implement", "1").unwrap();
    assert_eq!((e.scope, e.model.unwrap().value.as_str()), (Scope::Rig, "opus"));
    let text = format!(
        "scope = \"root\"\n{}",
        BASE.iter().find(|(p, _)| *p == agent).unwrap().1
    );
    let config = load_with(&[(agent, &text), ("shop/aae.toml", &rig_wide)]).unwrap();
    let (_, on_rig) = config.for_run("shop", "backend", "kit.implement", "1").unwrap();
    let (_, in_root) = config.for_run("shop", "root", "kit.implement", "1").unwrap();
    assert_eq!(on_rig, in_root, "the rig's settings do not reach an agent of the root");
    assert_eq!(on_rig.scope, Scope::Root);
    assert!(on_rig.explain().contains("scope = \"root\"  # agent"));
    let alone = config.effective("shop", Some("backend"), &implementer()).unwrap();
    assert_eq!(alone, config.effective("shop", None, &implementer()).unwrap());
    fails(&[(agent, "scope = \"repo\"\n")], "`scope` is `rig` or `root`");
}

/// A rig that edits an agent of the root, or maps it to a variant, is refused: neither would reach the agent.
#[test]
fn a_rig_neither_edits_nor_maps_an_agent_of_the_root() {
    let agent = "kit/agents/implementer/agent.toml";
    let text = format!(
        "scope = \"root\"\n{}",
        BASE.iter().find(|(p, _)| *p == agent).unwrap().1
    );
    let edit = load_with(&[(agent, &text)]).expect_err("the rig edits an agent of the root");
    assert!(
        edit.iter()
            .any(|e| e.message.contains("agent `implementer` works in the root")),
        "{edit:?}"
    );
    let mapped = BASE[3].1.replace(
        "[rigs.backend.agents.implementer]\nmodel = \"opus\"\n",
        "agents = { implementer = \"fast\" }\n[agents.fast]\nbase = \"kit.implementer\"\n",
    );
    let errors = load_with(&[(agent, &text), ("shop/aae.toml", &mapped)]).expect_err("the rig maps it");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("agent `kit.implementer` works in the root")),
        "{errors:?}"
    );
    let variant = BASE[3]
        .1
        .replace("[rigs.backend.agents.implementer]\n", "[rigs.backend.agents.fast]\n")
        + "[agents.fast]\nbase = \"kit.implementer\"\n";
    let errors = load_with(&[(agent, &text), ("shop/aae.toml", &variant)]).expect_err("the rig edits its variant");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("agent `fast` works in the root")),
        "{errors:?}"
    );
}

fn implementer() -> Name {
    Name {
        pack: "kit".into(),
        name: "implementer".into(),
    }
}

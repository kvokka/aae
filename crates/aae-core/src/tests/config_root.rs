//! The project root: its name and path, and the rigs beside it (spec §6.1, decision 99).

use super::{
    config::{BASE, fails, load_for_adapters, load_with},
    config_launch::{ADAPTERS, HARNESSES},
};

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

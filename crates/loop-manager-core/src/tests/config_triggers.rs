use std::fs;

/// The triggers the core lists for `shop`: a rig that lists the pack's schedule and a project trigger; `root` stands
/// before the rig, `rig` ends the trigger.
fn places(root: &str, rig: &str) -> Vec<String> {
    let project = format!(
        "packs = [\"../kit\"]\n{root}[rigs.backend]\npath = \"backend\"\ntriggers = [\"nightly\"]\n\
         [[trigger]]\nname = \"implement-next\"\non = \"bead.ready\"\nrun = \"audit\"\n{rig}"
    );
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("config.toml", "projects = [\"shop\"]\n"),
        ("shop/loop-manager.toml", project.as_str()),
        ("shop/backend/.keep", ""),
        (
            "kit/pack.toml",
            "name = \"kit\"\nversion = \"0.1.0\"\n[[trigger]]\nname = \"nightly\"\ncron = \"0 3 * * *\"\nrun = \"audit\"\n",
        ),
        (
            "kit/formulas/audit.toml",
            "name = \"audit\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
        ),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let config = crate::config::load_for(&dir.path().join("config.toml"), &[], Default::default()).unwrap();
    let db = dir.path().join("state.db");
    crate::open(&db).unwrap();
    let listed = crate::run::Reader::open(&db).unwrap().triggers(&config).unwrap();
    let mut at: Vec<String> = (listed.iter())
        .map(|t| format!("{} {}", t["trigger"], t["rig"]))
        .collect();
    at.sort();
    at
}

/// Where each trigger runs (docs/design/contracts.md 6): a project trigger without a `rig` in the root, under the
/// root's name, and with `rig` naming the root there too; a pack trigger only on the rigs that list it.
#[test]
fn a_project_trigger_without_a_rig_runs_in_the_root() {
    let expect = |root: &str| {
        vec![
            r#""kit.nightly" "backend""#.to_owned(),
            format!(r#""shop.implement-next" "{root}""#),
        ]
    };
    assert_eq!(places("", ""), expect("root"));
    let named = "[root]\nname = \"main\"\n";
    assert_eq!(places(named, ""), expect("main"));
    assert_eq!(places(named, "rig = \"main\"\n"), expect("main"));
}

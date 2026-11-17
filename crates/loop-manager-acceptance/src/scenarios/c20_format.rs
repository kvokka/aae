//! Criterion 20, without a model: the `format` step of demo 2, word for word from its pack, on a rig inside a git
//! repository that ignores the rig, as the demo lays it out. rumdl fixes what it can and passes; what it cannot fails
//! the step with rumdl's findings as the remarks; what tools fetch into the rig is not checked.

use crate::{
    Env,
    checks::root,
    daemon::{Daemon, write},
};
use std::{fs, process::Command, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

/// A reference with no definition: rumdl cannot fix it (MD052).
const UNFIXABLE: &str = "# Notes\n\nSee [the docs][missing].\n";

pub(crate) async fn run(env: Arc<Env>) -> String {
    let pack = root().join("demos/2-multirepo-qa/pack/formulas/deliver.toml");
    let pack = fs::read_to_string(pack).expect("the formula of demo 2");
    let format = pack
        .split("[[step]]")
        .find(|step| step.contains("name = \"format\""))
        .expect("a format step in demo 2");
    let d = Daemon::start(&env, |root| {
        let project = root.join("config/loop-manager/shop");
        write(project.join(".gitignore"), "/web/*\n");
        let git = Command::new("git").args(["init", "-q"]).current_dir(&project).status();
        assert!(git.is_ok_and(|s| s.success()), "git init");
        write(project.join("web/README.md"), "# Web\n\nTrailing spaces.   \n");
        write(project.join("web/node_modules/x/README.md"), UNFIXABLE);
        let plan = "[[step]]\nname = \"plan\"\nkind = \"exec\"\ncommand = [\"true\"]\n";
        write(
            project.join("formulas/format.toml"),
            format!("name = \"format\"\nrounds = 1\n{plan}[[step]]{format}"),
        );
    })
    .await;

    let run = d.create("format").await;
    let view = d.until_state(&run, "passed", WITHIN).await;
    let readme = fs::read_to_string(d.path("config/loop-manager/shop/web/README.md")).unwrap();
    assert_eq!(readme, "# Web\n\nTrailing spaces.\n", "{view}");

    write(d.path("config/loop-manager/shop/web/notes.md"), UNFIXABLE);
    let run = d.create("format").await;
    let view = d.until_state(&run, "failed", WITHIN).await;
    let reason = view["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("notes.md") && reason.contains("MD052") && !reason.contains("node_modules"),
        "{view}"
    );
    "fixed a rig the repository ignores and passed; an unfixable file failed the step with rumdl's findings".into()
}

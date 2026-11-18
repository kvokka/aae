//! The `if` of a step against a real `loop-manager daemon` (docs/design/config.md, docs/api/frontends.md): a step whose `if`
//! does not hold is skipped, and `loop-manager run watch` names it.

use crate::common::{self, Daemon, WAIT};
use std::fs;

#[tokio::test]
async fn a_skipped_step_shows_in_run_watch() {
    let d = Daemon::start(|dir| {
        let formula = "name = \"note\"\n[inputs]\nnotify = { type = \"boolean\" }\n[[step]]\nname = \"work\"\n\
                       kind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n[[step]]\nname = \"notify\"\n\
                       kind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\nif = { \"inputs.notify\" = true }\n";
        fs::write(dir.join("config/loop-manager/shop/formulas/note.toml"), formula).unwrap();
    })
    .await;
    let run = d.create("note", None).await;
    let view = d.until_state(&run, "passed").await;
    assert!(view["steps"].get("notify").is_none(), "{view}");
    assert_eq!(view["outcome"]["summary"], "exit 0", "{view}");
    // The run is over, so the stream ends with its history.
    let out = common::program("timeout")
        .arg(WAIT.as_secs().to_string())
        .arg(common::EXE)
        .args(["run", "watch", &run])
        .env("LOOP_MANAGER_SOCKET", d.socket())
        .env("HOME", d.dir.path())
        .env("XDG_CONFIG_HOME", d.dir.path().join("config"))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
    let skipped: Vec<&str> = text.lines().filter(|l| l.contains("run.step_skipped")).collect();
    assert_eq!(skipped.len(), 1, "{text}");
    assert!(
        skipped[0].ends_with(&format!("  run.step_skipped  {run}  step=notify")),
        "{text}"
    );
}

//! Criterion 29: a step whose check script fails twice and passes the third time passes at `max_attempts = 3` and
//! fails at `max_attempts = 2`; exit 75 of the script fails the step with no repeat; a repeat gets the script's
//! output as its remark; `kill -9` of the daemon between the attempts keeps the count.

use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const WITHIN: Duration = Duration::from_secs(60);

/// Formula `name` of project `shop`: one exec step `work` with `max_attempts = attempts` and the check script
/// `<name>.sh`, which counts its calls in `<name>.checks` and runs `verdict` with `$n` set. Unless `hold` is false,
/// the second execution of `work` creates `<name>.reached` and waits for `<name>.go`.
fn formula(root: &Path, name: &str, attempts: u32, verdict: &str, hold: bool) {
    let dir = root.join("config/loop-manager/shop/formulas");
    let at = |file: &str| root.join(file).display().to_string();
    let (runs, log, reached, go) = (
        at(&format!("{name}.runs")),
        at(&format!("{name}.log")),
        at(&format!("{name}.reached")),
        at(&format!("{name}.go")),
    );
    let mut work = format!(
        "n=$(($(cat '{runs}' 2>/dev/null || echo 0) + 1)); echo $n > '{runs}'; printf '[%s]\\n' \"$LOOP_MANAGER_REMARKS\" >> '{log}'"
    );
    if hold {
        work += &format!("; if [ $n = 2 ]; then touch '{reached}'; while [ ! -e '{go}' ]; do sleep 0.05; done; fi");
    }
    let checks = at(&format!("{name}.checks"));
    let script = dir.join(format!("{name}.sh"));
    write(
        &script,
        format!("#!/bin/sh\nn=$(($(cat '{checks}' 2>/dev/null || echo 0) + 1)); echo $n > '{checks}'\n{verdict}\n"),
    );
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let text = format!(
        "name = \"{name}\"\n[[step]]\nname = \"work\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {}]\n\
         max_attempts = {attempts}\nstep_check_script_path = \"{name}.sh\"\n",
        serde_json::json!(work)
    );
    write(dir.join(format!("{name}.toml")), text);
}

/// The calls of the check script of formula `name`.
fn calls(d: &Daemon, name: &str) -> u32 {
    let text = fs::read_to_string(d.path(&format!("{name}.checks"))).unwrap_or_default();
    text.trim().parse().unwrap_or(0)
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let third_time = "[ $n -ge 3 ] || { echo 'not yet'; exit 1; }";
    let mut d = Daemon::start(&env, |root| {
        write(
            root.join("config/loop-manager/config.toml"),
            "projects = [\"shop\"]\nmax_runs = 1\n",
        );
        formula(root, "third", 3, third_time, true);
        formula(root, "second", 2, third_time, true);
        formula(root, "fatal", 3, "echo 'broken for good'; exit 75", false);
    })
    .await;
    let (third, second, fatal) = (
        d.create("third").await,
        d.create("second").await,
        d.create("fatal").await,
    );

    // `max_runs = 1` runs them one at a time: each is killed in its own turn, between its first and second check.
    for name in ["third", "second"] {
        let deadline = Instant::now() + WITHIN;
        while !d.path(&format!("{name}.reached")).exists() {
            assert!(Instant::now() < deadline, "no second execution of `work` in {name}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(calls(&d, name), 1, "one check of {name} before the kill");
        d.restart().await;
        write(d.path(&format!("{name}.go")), "");
    }

    let view = d.until_state(&third, "passed", WITHIN).await;
    assert_eq!(calls(&d, "third"), 3, "three checks across the kill: {view}");
    assert_eq!(view["step_attempt"], 3, "{view}");
    let log = fs::read_to_string(d.path("third.log")).expect("the log of work");
    assert!(log.starts_with("[]\n"), "{log}");
    assert!(
        log.ends_with("[work: check script: exit 1\nnot yet]\n"),
        "the remark of a repeat: {log}"
    );

    let view = d.until_state(&second, "failed", WITHIN).await;
    assert_eq!(calls(&d, "second"), 2, "two checks, no third: {view}");
    assert_eq!(view["step_attempt"], 2, "{view}");
    let view = d.until_state(&fatal, "failed", WITHIN).await;
    assert_eq!(calls(&d, "fatal"), 1, "exit 75 repeats nothing: {view}");
    assert_eq!(view["step_attempt"], 1, "{view}");
    "max_attempts 3 passed on the third check across kill -9, 2 failed after two; exit 75 failed after one".into()
}

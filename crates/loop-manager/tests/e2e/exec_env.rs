//! A secret given by reference (docs/design/config.md, "Environment values"): the daemon resolves `${NAME}` when the
//! process starts and keeps it nowhere.

use crate::common::{Daemon, WAIT, lock_once_journalled};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MARKER: &str = "s3cr3t-8f41c2d7e9a05b36";

/// The files under `dir`, at any depth, whose bytes hold `needle`.
fn holding(dir: &Path, needle: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let (path, kind) = (entry.path(), entry.file_type().unwrap());
        if kind.is_dir() {
            found.extend(holding(&path, needle));
        } else if kind.is_file() && fs::read(&path).is_ok_and(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
        {
            found.push(path.display().to_string());
        }
    }
    found
}

#[tokio::test]
async fn a_secret_reaches_the_step_and_is_stored_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "config.toml",
            "projects = [\"shop\"]\n[harness.claude]\nbinary = \"true\"\n[exec]\nenv = { TOKEN = \"${LOOP_MANAGER_E2E_SECRET}\" }\n",
        ),
        ("shop/loop-manager.toml", "[rigs.web]\npath = \"web\"\n"),
        ("shop/web/.keep", ""),
        (
            "shop/agents/dev/agent.toml",
            "harness = \"claude\"\nenv = { GH_TOKEN = \"${LOOP_MANAGER_E2E_SECRET}\" }\n",
        ),
        (
            "shop/formulas/leak.toml",
            "name = \"leak\"\n[[step]]\nkind = \"exec\"\n\
             command = [\"sh\", \"-c\", \"printf %s \\\"$TOKEN\\\" > token.out\"]\ntimeout = \"10s\"\n",
        ),
    ];
    for (path, text) in files {
        let path = dir.path().join("config/loop-manager").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let log = fs::File::create(dir.path().join("daemon.log")).unwrap();
    let child = crate::common::command(dir.path())
        .env("LOOP_MANAGER_E2E_SECRET", MARKER)
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let client = loop_manager_front::client::Client::new(dir.path().join("run/loop-manager/loop-manager.sock"));
    let d = Daemon { child, dir, client };
    let deadline = Instant::now() + WAIT;
    while d.client.request("GET", "/v1/daemon", None, None).await.is_err() {
        assert!(Instant::now() < deadline, "the daemon did not come up");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let run = d.create("leak", None).await;
    d.until_state(&run, "passed").await;
    let out = d.dir.path().join("config/loop-manager/shop/web/token.out");
    assert_eq!(fs::read_to_string(&out).unwrap(), MARKER, "the step got the secret");
    fs::remove_file(&out).unwrap();
    let cli = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_loop-manager"))
            .args(args)
            .env("LOOP_MANAGER_SOCKET", d.socket())
            .env("HOME", d.dir.path())
            .env("XDG_CONFIG_HOME", d.dir.path().join("config"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let explain = |what: &[&str]| cli(&[&["config", "explain", "--project", "shop"][..], what].concat());
    let explained = [
        explain(&["--formula", "leak", "--rig", "web"]),
        explain(&["--agent", "dev"]),
    ];
    for text in &explained {
        assert!(text.contains("\"${LOOP_MANAGER_E2E_SECRET}\""), "{text}");
    }
    let shown = [cli(&["run", "show", &run]), cli(&["run", "show", &run, "--json"])];
    for text in explained.iter().chain(&shown) {
        assert!(!text.contains(MARKER), "{text}");
    }
    // Once the journal has every event, nothing in the tree holds the secret: state with its WAL, journal, log.
    let db = lock_once_journalled(d.dir.path()).await;
    assert_eq!(holding(d.dir.path(), MARKER.as_bytes()), Vec::<String>::new());
    drop(db);
}

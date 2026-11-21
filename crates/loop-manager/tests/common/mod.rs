//! A real `loop-manager daemon` in a temporary XDG tree with exec formulas, so no harness is needed.

pub(crate) mod schema;

use loop_manager_front::client::{Client, ClientError};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

pub(crate) const FILES: &[(&str, &str)] = &[
    ("config/loop-manager/config.toml", "projects = [\"shop\"]\n"),
    (
        "config/loop-manager/shop/loop-manager.toml",
        "[rigs.web]\npath = \"web\"\n",
    ),
    ("config/loop-manager/shop/web/.keep", ""),
    (
        "config/loop-manager/shop/formulas/check.toml",
        "name = \"check\"\n[inputs]\nn = { type = \"integer\" }\n[[step]]\nkind = \"exec\"\ncommand = \
        [\"true\"]\ntimeout = \"10s\"\n",
    ),
    (
        "config/loop-manager/shop/formulas/slow.toml",
        "name = \"slow\"\n[[step]]\nkind = \"exec\"\ncommand = [\"sleep\", \"30\"]\ntimeout = \"60s\"\n",
    ),
];

/// The built `loop-manager`. Tests name it only through here and start it only through [`program`]: they may run
/// inside a session of an outer loop-manager (docs/agents/rules-tests.md).
pub(crate) const EXE: &str = env!("CARGO_BIN_EXE_loop-manager");

/// `program` without the environment of an outer loop-manager session the tests may run in.
pub(crate) fn program(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    loop_manager_testkit::without_outer_session(&mut command);
    command
}

/// Test daemons and standalone servers also go away if the test runner is killed.
pub(crate) fn owned_program(path: PathBuf) -> Command {
    let mut command = loop_manager();
    command
        .arg("exec-with-parent")
        .arg(std::process::id().to_string())
        .arg("--")
        .arg(path);
    command
}

/// The built `loop-manager`, through [`program`].
pub(crate) fn loop_manager() -> Command {
    program(EXE)
}

/// The upper bound of every wait on the daemon: generous, so that a loaded machine is slow rather than failing.
pub(crate) const WAIT: Duration = Duration::from_secs(60);

pub(crate) struct Daemon {
    pub(crate) child: Child,
    pub(crate) dir: tempfile::TempDir,
    pub(crate) client: Client,
}

impl Daemon {
    pub(crate) fn socket(&self) -> PathBuf {
        self.dir.path().join("run/loop-manager/loop-manager.sock")
    }

    /// `loop-manager daemon` with the XDG directories in a temporary one; `before` prepares it.
    pub(crate) async fn start(before: impl FnOnce(&std::path::Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        before(dir.path());
        let child = spawn(dir.path());
        let client = Client::new(dir.path().join("run/loop-manager/loop-manager.sock"));
        let daemon = Self { child, dir, client };
        let deadline = Instant::now() + WAIT;
        while daemon.client.request("GET", "/v1/daemon", None, None).await.is_err() {
            assert!(Instant::now() < deadline, "the daemon did not come up");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        daemon
    }

    pub(crate) async fn get(&self, path: &str) -> Value {
        self.client.request("GET", path, None, None).await.unwrap()
    }

    pub(crate) async fn post(&self, path: &str, body: Value, key: Option<&str>) -> Result<Value, ClientError> {
        self.client.request("POST", path, Some(&body), key).await
    }

    pub(crate) async fn create(&self, formula: &str, key: Option<&str>) -> String {
        let body = json!({ "project": "shop", "rig": "web", "formula": formula });
        let reply = self.post("/v1/runs", body, key).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// Waits until the run is in `state`.
    pub(crate) async fn until_state(&self, run: &str, state: &str) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let view = self.get(&format!("/v1/runs/{run}")).await;
            if view["state"] == state {
                return view;
            }
            assert!(Instant::now() < deadline, "{run} stays {}", view["state"]);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub(crate) fn terminate(&mut self) -> ExitStatus {
        let pid = self.child.id().to_string();
        assert!(Command::new("kill").args(["-TERM", &pid]).status().unwrap().success());
        self.child.wait().unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn spawn(dir: &std::path::Path) -> Child {
    command(dir).spawn().unwrap()
}

/// `loop-manager daemon` in the XDG tree under `dir`: the binary `dir/bin/loop-manager` if a test put one there, else the built one.
pub(crate) fn command(dir: &std::path::Path) -> Command {
    let own = dir.join("bin/loop-manager");
    let mut command = owned_program(if own.exists() { own } else { EXE.into() });
    command
        .arg("daemon")
        .env("HOME", dir)
        .env_remove("CODEX_HOME")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("run"));
    // A test's own tools in `dir/path` go first on the PATH of the daemon and of what it runs.
    let tools = dir.join("path");
    if tools.is_dir() {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let paths = std::iter::once(tools).chain(std::env::split_paths(&path));
        command.env("PATH", std::env::join_paths(paths).unwrap());
    }
    command
}

/// Holds the database of the daemon in `dir` in an exclusive transaction, taken once the journal has every event:
/// its retried writes would otherwise each wait out the busy timeout before the core's next write fails.
pub(crate) async fn lock_once_journalled(dir: &std::path::Path) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(dir.join("state/loop-manager/state.db")).unwrap();
    let behind = "SELECT EXISTS (SELECT 1 FROM outbox WHERE seq > (SELECT seq FROM consumers WHERE name = 'journal'))";
    let deadline = Instant::now() + WAIT;
    loop {
        db.execute_batch("BEGIN EXCLUSIVE").unwrap();
        if !db.query_row(behind, [], |r| r.get::<_, bool>(0)).unwrap() {
            return db;
        }
        db.execute_batch("ROLLBACK").unwrap();
        assert!(
            Instant::now() < deadline,
            "the journal did not catch up with the outbox in {WAIT:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

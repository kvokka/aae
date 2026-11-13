//! A real `aae daemon` in a temporary XDG tree with exec formulas, so no harness is needed.
// Each test crate uses a part of this module.
#![allow(dead_code)]

use aae_front::client::{Client, ClientError};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

pub(crate) const FILES: &[(&str, &str)] = &[
    ("config/aae/config.toml", "projects = [\"shop\"]\n"),
    ("config/aae/shop/aae.toml", "[rigs.web]\npath = \"web\"\n"),
    ("config/aae/shop/web/.keep", ""),
    (
        "config/aae/shop/formulas/check.toml",
        "name = \"check\"\n[inputs]\nn = { type = \"integer\" }\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
    ),
    (
        "config/aae/shop/formulas/slow.toml",
        "name = \"slow\"\n[step]\nkind = \"exec\"\ncommand = [\"sleep\", \"30\"]\ntimeout = \"60s\"\n",
    ),
];

/// The upper bound of every wait on the daemon: generous, so that a loaded machine is slow rather than failing.
pub(crate) const WAIT: Duration = Duration::from_secs(60);

pub(crate) struct Daemon {
    pub(crate) child: Child,
    pub(crate) dir: tempfile::TempDir,
    pub(crate) client: Client,
}

impl Daemon {
    pub(crate) fn socket(&self) -> PathBuf {
        self.dir.path().join("run/aae/aae.sock")
    }

    /// `aae daemon` with the XDG directories in a temporary one; `before` prepares it.
    pub(crate) async fn start(before: impl FnOnce(&std::path::Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        before(dir.path());
        let child = spawn(dir.path());
        let client = Client::new(dir.path().join("run/aae/aae.sock"));
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

/// `aae daemon` in the XDG tree under `dir`.
pub(crate) fn command(dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
    command
        .arg("daemon")
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("run"));
    command
}

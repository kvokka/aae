//! A real `aae daemon` with its XDG directories and `HOME` in a temporary tree: project `shop` with rig `web`.

use crate::Env;
use aae_front::client::{Client, ClientError};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Arc,
    time::{Duration, Instant},
};

const FILES: &[(&str, &str)] = &[
    ("config/aae/config.toml", "projects = [\"shop\"]\n"),
    ("config/aae/shop/aae.toml", "[rigs.web]\npath = \"web\"\n"),
    ("config/aae/shop/web/.keep", ""),
];

/// Writes `text` to `path`, creating its directories.
pub(crate) fn write(path: impl AsRef<Path>, text: impl AsRef<str>) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().expect("a file path")).unwrap();
    fs::write(path, text.as_ref()).unwrap();
}

pub(crate) struct Daemon {
    env: Arc<Env>,
    home: PathBuf,
    child: Child,
    pub(crate) dir: tempfile::TempDir,
    pub(crate) client: Client,
}

impl Daemon {
    /// A daemon on a fresh tree, `HOME` included; `before` adds to the tree before the daemon starts.
    pub(crate) async fn start(env: &Arc<Env>, before: impl FnOnce(&Path)) -> Self {
        Self::launch(env, before, None).await
    }

    /// As [`Daemon::start`], but with the user's `HOME`: agents that inherit the host find the host login there.
    pub(crate) async fn start_on_host(env: &Arc<Env>, before: impl FnOnce(&Path)) -> Self {
        let home = std::env::var_os("HOME").expect("HOME is set");
        Self::launch(env, before, Some(home.into())).await
    }

    async fn launch(env: &Arc<Env>, before: impl FnOnce(&Path), home: Option<PathBuf>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in FILES {
            write(dir.path().join(path), text);
        }
        before(dir.path());
        let home = home.unwrap_or_else(|| dir.path().to_owned());
        let child = command(env, dir.path(), &home).arg("daemon").spawn().unwrap();
        let client = Client::new(dir.path().join("run/aae/aae.sock"));
        let daemon = Self {
            env: env.clone(),
            home,
            child,
            dir,
            client,
        };
        daemon.until_up().await;
        daemon
    }

    /// `aae <args>` as a client of this daemon.
    pub(crate) fn cli(&self, args: &[&str]) -> std::process::Output {
        command(&self.env, self.dir.path(), &self.home)
            .args(args)
            .output()
            .unwrap()
    }

    pub(crate) fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    async fn until_up(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.client.request("GET", "/v1/daemon", None, None).await.is_err() {
            assert!(Instant::now() < deadline, "the daemon did not come up");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Kills the daemon with SIGKILL and starts it again on the same tree.
    pub(crate) async fn restart(&mut self) {
        self.restart_after(Duration::ZERO).await;
    }

    /// Kills the daemon with SIGKILL and starts it again on the same tree after `down`.
    pub(crate) async fn restart_after(&mut self, down: Duration) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        tokio::time::sleep(down).await;
        self.child = command(&self.env, self.dir.path(), &self.home)
            .arg("daemon")
            .spawn()
            .unwrap();
        self.until_up().await;
    }

    pub(crate) async fn get(&self, path: &str) -> Value {
        self.client.request("GET", path, None, None).await.unwrap()
    }

    pub(crate) async fn post(&self, path: &str, body: Value) -> Result<Value, ClientError> {
        self.client.request("POST", path, Some(&body), None).await
    }

    /// Starts `formula` on rig `web` of `shop`; the id of the run.
    pub(crate) async fn create(&self, formula: &str) -> String {
        let body = json!({ "project": "shop", "rig": "web", "formula": formula });
        let reply = self.post("/v1/runs", body).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// Waits up to `within` until the run is in `state`; its view.
    pub(crate) async fn until_state(&self, run: &str, state: &str, within: Duration) -> Value {
        let deadline = Instant::now() + within;
        loop {
            let view = self.get(&format!("/v1/runs/{run}")).await;
            if view["state"] == state {
                return view;
            }
            let ended = ["passed", "failed", "cancelled"].iter().any(|s| view["state"] == *s);
            assert!(!ended, "{run} ended, not {state}: {view}");
            assert!(Instant::now() < deadline, "{run} stays {}, not {state}", view["state"]);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Daemon {
    /// The journal from its start as `(type, envelope)`, read until `done` holds for what was read, up to `within`.
    pub(crate) async fn journal_until(
        &self,
        within: Duration,
        mut done: impl FnMut(&[(String, Value)]) -> bool,
    ) -> Vec<(String, Value)> {
        let deadline = tokio::time::Instant::now() + within;
        let mut events = self.client.events(Some(1)).await.unwrap();
        let mut seen = Vec::new();
        while !done(&seen) {
            let next = tokio::time::timeout_at(deadline, events.next()).await;
            let Ok(Some(event)) = next.map(Result::unwrap) else {
                panic!("the journal did not get there within {within:?}: {seen:?}");
            };
            seen.push((event.event.clone(), serde_json::from_str(&event.data).unwrap()));
        }
        seen
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `aae` with the XDG directories in `dir`.
fn command(env: &Env, dir: &Path, home: &Path) -> Command {
    let mut command = Command::new(&env.aae);
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("run"));
    command
}

//! `loop-manager daemon`: the core and the API on a unix socket, in the foreground (docs/guide/install.md), until SIGTERM or
//! SIGINT.

use crate::harness::{binary, driver, load, minimum};
use loop_manager_br::Br;
use loop_manager_core::{
    Clock, CoreHandle, Journal,
    config::Config,
    driver::Driver,
    open,
    run::{self, ExecHooks, Msg, Reader},
    tracker::Tracker,
};
use loop_manager_front::{
    api::{self, Api},
    dirs,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{DirBuilder, File, Permissions, TryLockError},
    io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use tokio::{
    net::{TcpListener, UnixListener, UnixStream},
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};

/// Where the daemon keeps its files (XDG base directories).
#[derive(Clone)]
pub(crate) struct Paths {
    pub(crate) config: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) socket: PathBuf,
    /// The packs and addons downloaded from git.
    pub(crate) cache: PathBuf,
    /// This binary, taken at start: once it is replaced on disk, Linux names the running one `<path> (deleted)`.
    pub(crate) exe: PathBuf,
}

impl Paths {
    /// The XDG locations, with `--config` and `--socket` taking precedence.
    pub(crate) fn resolve(config: Option<PathBuf>, socket: Option<PathBuf>) -> Result<Self, String> {
        let config = match config {
            // Only the default file may be missing (no config, the defaults); a named one must be there. Made
            // absolute here once: the config's relative paths resolve against its directory, whatever the cwd.
            Some(config) => {
                std::fs::metadata(&config).map_err(|e| format!("{}: {e}", config.display()))?;
                std::path::absolute(&config).map_err(|e| format!("{}: {e}", config.display()))?
            }
            None => dirs::xdg("XDG_CONFIG_HOME", ".config")?.join("loop-manager/config.toml"),
        };
        let socket = match socket {
            Some(socket) => socket,
            None => dirs::socket()?,
        };
        Ok(Self {
            config,
            state: dirs::state()?,
            socket,
            cache: dirs::cache()?,
            exe: std::env::current_exe().unwrap_or_else(|_| "loop-manager".into()),
        })
    }
}

/// The directory of each declared addon's data, `${LOOP_MANAGER_ADDON_DATA}` (docs/design/config.md): made, never read.
fn addon_data(config: &Config) -> Result<(), String> {
    let Some(data) = &config.addon_data else {
        return Ok(());
    };
    for addon in (config.addons.iter()).chain(config.projects.iter().flat_map(|p| &p.addons)) {
        let dir = data.join(&addon.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Runs the daemon until a stop signal or a drain; `true` when the drain was a restart. An error is why it could
/// not start.
pub(crate) async fn run(paths: Paths) -> Result<bool, String> {
    let mut config = load(&paths).map_err(|errors| format!("the config has errors:\n{}", errors.join("\n")))?;
    private_dir(&paths.state)?;
    addon_data(&config)?;
    // Held until the daemon exits: one writer per state directory (docs/design/event-model.md), whatever socket it
    // serves.
    let _lock = lock(&paths.state)?;
    // Daemons before the lock on the directory left a `lock` file (docs/design/event-model.md: no lock files): it goes.
    let _ = std::fs::remove_file(paths.state.join("lock"));
    let db = paths.state.join("state.db");
    let conn = open(&db).map_err(|e| format!("{}: {e}", db.display()))?;
    let journal_dir = paths.state.join("journal");
    let journal = Journal::new(&journal_dir);
    let mut written = journal.subscribe();
    let journal_health = journal.health();
    let listener = bind(&paths.socket).await?;
    let tcp = bind_tcp(&mut config, &paths.state).await?;
    // Descendants of instances that left their group are re-parented here and reaped (invariant 7).
    loop_manager_core::orphans::adopt_orphans().map_err(|e| format!("cannot become the child subreaper: {e}"))?;
    let mut drivers: Vec<Arc<dyn Driver>> = Vec::new();
    for (name, harness) in &config.harnesses {
        let Some((driver, binary, _)) = driver(name, harness, &paths) else {
            continue;
        };
        if let Err(why) = loop_manager_core::supported_version(&binary, minimum(&driver)) {
            eprintln!("loop-manager daemon: warning: {why}");
        }
        drivers.push(driver);
    }
    // A project with a `.beads` directory has br as its tracker (docs/design/tracker-br.md).
    let beads: Vec<(String, PathBuf)> = config
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.dir.join(".beads")))
        .filter(|(_, dir)| dir.is_dir())
        .collect();
    let trackers = trackers(&paths.config, &beads);
    let config = Arc::new(config);
    let (core, mut task) = run::start(
        conn,
        Clock::system(),
        config.clone(),
        drivers.clone(),
        trackers,
        Arc::new(ExecHooks {
            socket: Some(paths.socket.clone()),
            bundled: Some(loop_manager_addons::run),
        }),
        journal,
    );
    let watchers = watch_trackers(core.clone(), beads).await;
    let token = config.api_token.clone();
    let api = Api {
        core: core.clone(),
        reader: Reader::open(&db).map_err(|e| format!("{}: {e}", db.display()))?,
        journal: Journal::new(&journal_dir),
        written: written.clone(),
        journal_health,
        stop: tokio::sync::watch::channel(None).0,
        stopping: std::sync::OnceLock::new(),
        daemon: json!({
            "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
            "socket": paths.socket, "started_at": loop_manager_core::rfc3339_ms(Clock::system().now_ms()),
        }),
        config: RwLock::new(config),
        applied: tokio::sync::Mutex::new(1),
        load: Box::new({
            let paths = paths.clone();
            move || reload(&paths, &token)
        }),
        drivers,
        doctor: Box::new({
            let paths = paths.clone();
            move || crate::doctor::findings(&paths)
        }),
    };
    let stop = stop_signal().map_err(|e| format!("signals: {e}"))?;
    eprintln!("loop-manager daemon listening on {}", paths.socket.display());
    // A core that stopped leaves an API that can only fail: the daemon exits with an error, for a supervisor to see.
    let served = tokio::select! {
        served = api::serve(listener, tcp, api, stop) => served,
        died = &mut task => {
            let _ = std::fs::remove_file(&paths.socket);
            let why = died.map_or_else(|e| e.to_string(), |r| r.err().map_or("no error".into(), |e| e.to_string()));
            return Err(format!("the core stopped: {why}"));
        }
    };
    let _ = std::fs::remove_file(&paths.socket);
    // `daemon.stopping` goes to the journal before the exit; a drain emitted it when it began, so this repeat only
    // reads its `seq`. The writer is then left as a crash leaves it: the state is crash-safe.
    let reason = served.as_ref().map_or("signal", |r| r);
    let stopping = Msg::Stopping {
        reason: reason.to_owned(),
    };
    if let Ok(reply) = core.send(None, stopping).await {
        let seq = reply["seq"].as_i64().unwrap_or_default();
        let flushed = written.wait_for(|w| *w >= seq);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), flushed).await;
    }
    // The core goes first, so no effect records the end of a process killed below, and none is cancelled under the
    // writer as the runtime shuts down. Then what the daemon started goes with it, as the next start would end it.
    task.abort();
    let _ = task.await;
    drop(watchers);
    loop_manager_core::orphans::kill_descendants();
    served
        .map(|reason| reason == "restart")
        .map_err(|e| format!("serving the API: {e}"))
}

async fn bind_tcp(config: &mut Config, state: &Path) -> Result<Option<TcpListener>, String> {
    let Some(addr) = config.api_tcp else {
        return Ok(None);
    };
    // A new token every start, for the owner's clients in `api.token` and for sessions through `${LOOP_MANAGER_API}`.
    config.api_token = loop_manager_core::run::new_token().ok_or("no randomness for the API token")?;
    let file = state.join("api.token");
    // Created 0600 so the token is never readable by others, not even until a chmod.
    let _ = std::fs::remove_file(&file);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
        .and_then(|mut f| std::io::Write::write_all(&mut f, config.api_token.as_bytes()))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    TcpListener::bind(addr)
        .await
        .map(Some)
        .map_err(|e| format!("api_tcp {addr}: {e}"))
}

fn trackers(config_file: &Path, beads: &[(String, PathBuf)]) -> BTreeMap<String, Arc<dyn Tracker>> {
    let br = binary("br", None).0;
    let trackers = beads
        .iter()
        .map(|(name, dir)| {
            (
                name.clone(),
                Arc::new(Br::new(br.clone(), dir.clone())) as Arc<dyn Tracker>,
            )
        })
        .collect();
    if let Some(why) = (!beads.is_empty())
        .then(|| loop_manager_core::tested_version(&br, loop_manager_br::TESTED).err())
        .flatten()
    {
        eprintln!("loop-manager daemon: warning: {why}");
    }
    if let Err(e) = bv_workspace(config_file, &br, beads) {
        eprintln!("loop-manager daemon: warning: the bv workspace file: {e}");
    }
    trackers
}

/// Keeps filesystem watches active until the daemon drops the returned guard.
async fn watch_trackers(core: CoreHandle<Msg>, beads: Vec<(String, PathBuf)>) -> impl Drop {
    // Each watched write tells the core to read its project's tracker; a burst of writes is one read.
    let (changed, mut changes) = mpsc::unbounded_channel::<String>();
    let mut watchers = Vec::new();
    for (name, dir) in beads {
        let (changed, project) = (changed.clone(), name.clone());
        match loop_manager_br::watch(&dir, move || drop(changed.send(project.clone()))) {
            Ok(w) => watchers.push(w),
            Err(e) => {
                eprintln!("loop-manager daemon: warning: tracker of `{name}` is not watched: {e}");
                // A failure with no retry: the periodic resync alone reads this tracker.
                let payload = json!({ "call": "watch", "project": name });
                let result = Err(loop_manager_core::tracker::TrackerError::Invalid { text: e });
                let _ = core.send(None, Msg::Tracked { payload, result }).await;
            }
        }
    }
    tokio::spawn(async move {
        while let Some(first) = changes.recv().await {
            let mut projects = BTreeSet::from([first]);
            while let Ok(more) = changes.try_recv() {
                projects.insert(more);
            }
            for project in projects {
                let _ = core.send(None, Msg::TrackerChanged(project)).await;
            }
        }
    });
    watchers
}

fn reload(paths: &Paths, token: &str) -> Result<Config, Vec<String>> {
    // git may take seconds, and reload and check call this from a handler.
    let mut config = tokio::task::block_in_place(|| load(paths))?;
    token.clone_into(&mut config.api_token);
    addon_data(&config).map_err(|e| vec![e])?;
    Ok(config)
}

/// An exclusive lock on the state directory itself, so no lock file is left in it (docs/design/event-model.md); the OS
/// drops it with the process, so a crash leaves none behind.
fn lock(state: &Path) -> Result<File, String> {
    let dir = File::open(state).map_err(|e| format!("{}: {e}", state.display()))?;
    match dir.try_lock() {
        Ok(()) => Ok(dir),
        Err(TryLockError::WouldBlock) => Err(format!("another daemon owns {}", state.display())),
        Err(TryLockError::Error(e)) => Err(format!("{}: {e}", state.display())),
    }
}

fn private_dir(dir: &Path) -> Result<(), String> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))
}

/// Binds the socket in a private directory. A socket file nobody answers on is left by a dead daemon and goes;
/// one a daemon answers on means this one must not start.
async fn bind(socket: &Path) -> Result<UnixListener, String> {
    let shown = socket.display();
    if let Some(dir) = socket.parent() {
        private_dir(dir)?;
    }
    if socket.exists() {
        if UnixStream::connect(socket).await.is_ok() {
            return Err(format!("{shown}: another daemon is listening"));
        }
        std::fs::remove_file(socket).map_err(|e| format!("{shown}: {e}"))?;
    }
    let listener = UnixListener::bind(socket).map_err(|e| format!("{shown}: {e}"))?;
    // The directory may be a shared one given by `--socket`; the socket itself is the owner's only.
    std::fs::set_permissions(socket, Permissions::from_mode(0o600)).map_err(|e| format!("{shown}: {e}"))?;
    Ok(listener)
}

fn stop_signal() -> io::Result<impl Future<Output = ()> + Send + 'static> {
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    })
}

/// Writes `.bv/workspace.yaml` next to the orchestrator config: one repository per project with a tracker, so `bv`
/// run there (or with `--workspace`) shows every project's beads (docs/design/tracker-br.md). The trackers are fixed
/// for the daemon's life (a reload changing them is refused), so the file is written at start only.
fn bv_workspace(config: &Path, br: &Path, beads: &[(String, PathBuf)]) -> Result<(), String> {
    if beads.is_empty() {
        return Ok(());
    }
    let mut yaml =
        "# Written by loop-manager at daemon start from the project configs; edits are lost.\nname: loop-manager\nrepos:\n".to_owned();
    for (name, dir) in beads {
        let root = dir.parent().unwrap_or(dir);
        // JSON strings are YAML double-quoted scalars.
        yaml.push_str(&format!("  - name: {}\n    path: {}\n", json!(name), json!(root)));
        // The id prefix br was initialized with; without it bv assumes `<name>-`.
        match loop_manager_br::issue_prefix(br, dir) {
            Some(prefix) => yaml.push_str(&format!("    prefix: {}\n", json!(format!("{prefix}-")))),
            None => eprintln!("loop-manager daemon: warning: no br id prefix of `{name}`; bv assumes `{name}-`"),
        }
    }
    let dir = config.parent().unwrap_or(Path::new(".")).join(".bv");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("workspace.yaml");
    std::fs::write(&file, yaml).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;

//! `aae daemon`: the core and the API on a unix socket, in the foreground (spec §13), until SIGTERM or SIGINT.

use aae_br::Br;
use aae_claude::Claude;
use aae_codex::Codex;
use aae_core::{
    Clock, Journal, config,
    driver::{Driver, ProcessPart},
    open,
    run::{self, ExecHooks, Msg, Reader},
    tracker::Tracker,
};
use aae_front::{
    api::{self, Api},
    dirs,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{DirBuilder, File, Permissions, TryLockError},
    io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use tokio::{
    net::{UnixListener, UnixStream},
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};

/// Where the daemon keeps its files (XDG base directories).
pub(crate) struct Paths {
    pub(crate) config: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) socket: PathBuf,
}

impl Paths {
    /// The XDG locations, with `--config` and `--socket` taking precedence.
    pub(crate) fn resolve(config: Option<PathBuf>, socket: Option<PathBuf>) -> Result<Self, String> {
        let config = match config {
            Some(config) => config,
            None => dirs::xdg("XDG_CONFIG_HOME", ".config")?.join("aae/config.toml"),
        };
        let socket = match socket {
            Some(socket) => socket,
            None => dirs::socket()?,
        };
        Ok(Self {
            config,
            state: dirs::state()?,
            socket,
        })
    }
}

/// Runs the daemon until a stop signal or a drain; `true` when the drain was a restart. An error is why it could
/// not start.
pub(crate) async fn run(paths: Paths) -> Result<bool, String> {
    let mut config = load(&paths.config)
        .inspect(crate::doctor::warn_per_session)
        .map_err(|errors| format!("the config has errors:\n{}", errors.join("\n")))?;
    private_dir(&paths.state)?;
    // Held until the daemon exits: one writer per state directory (spec §5.1), whatever socket it serves.
    let _lock = lock(&paths.state)?;
    let db = paths.state.join("state.db");
    let conn = open(&db).map_err(|e| format!("{}: {e}", db.display()))?;
    let journal_dir = paths.state.join("journal");
    let journal = Journal::new(&journal_dir);
    let written = journal.subscribe();
    let journal_health = journal.health();
    let listener = bind(&paths.socket).await?;
    let tcp = match config.api_tcp {
        Some(addr) => {
            // A new token every start, for the owner's clients in `api.token` and for sessions through `${AAE_API}`.
            config.api_token = aae_core::run::new_token().ok_or("no randomness for the API token")?;
            let file = paths.state.join("api.token");
            std::fs::write(&file, &config.api_token)
                .and_then(|()| std::fs::set_permissions(&file, Permissions::from_mode(0o600)))
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let tcp = tokio::net::TcpListener::bind(addr).await;
            Some(tcp.map_err(|e| format!("api_tcp {addr}: {e}"))?)
        }
        None => None,
    };
    // Descendants of instances that left their group are re-parented here and reaped (invariant 7).
    aae_core::orphans::adopt_orphans().map_err(|e| format!("cannot become the child subreaper: {e}"))?;
    let mut drivers: Vec<Arc<dyn Driver>> = Vec::new();
    for (name, harness) in &config.harnesses {
        let Some((driver, binary, _)) = driver(name, harness, &paths) else {
            continue;
        };
        if let Some(why) = aae_core::untested(&binary, tested(&driver)) {
            eprintln!("aae daemon: warning: {why}");
        }
        drivers.push(driver);
    }
    // A project with a `.beads` directory has br as its tracker (spec §10.2).
    let beads: Vec<(String, PathBuf)> = config
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.root.join(".beads")))
        .filter(|(_, dir)| dir.is_dir())
        .collect();
    let br = binary("br", None).0;
    let trackers: BTreeMap<String, Arc<dyn Tracker>> = beads
        .iter()
        .map(|(name, dir)| {
            (
                name.clone(),
                Arc::new(Br::new(br.clone(), dir.clone())) as Arc<dyn Tracker>,
            )
        })
        .collect();
    if let Some(why) = (!beads.is_empty())
        .then(|| aae_core::untested(&br, aae_br::TESTED))
        .flatten()
    {
        eprintln!("aae daemon: warning: {why}");
    }
    let config = Arc::new(config);
    let (core, task) = run::start(
        conn,
        Clock::system(),
        config.clone(),
        drivers.clone(),
        trackers,
        Arc::new(ExecHooks {
            socket: Some(paths.socket.clone()),
            bundled: Some(aae_plugins::run),
        }),
        journal,
    );
    // Each watched write tells the core to read its project's tracker; a burst of writes is one read.
    let (changed, mut changes) = mpsc::unbounded_channel::<String>();
    let mut watchers = Vec::new();
    for (name, dir) in beads {
        let (changed, project) = (changed.clone(), name.clone());
        match aae_br::watch(&dir, move || drop(changed.send(project.clone()))) {
            Ok(w) => watchers.push(w),
            Err(e) => eprintln!("aae daemon: warning: tracker of `{name}` is not watched: {e}"),
        }
    }
    let notify = core.clone();
    tokio::spawn(async move {
        while let Some(first) = changes.recv().await {
            let mut projects = BTreeSet::from([first]);
            while let Ok(more) = changes.try_recv() {
                projects.insert(more);
            }
            for project in projects {
                let _ = notify.send(None, Msg::TrackerChanged(project)).await;
            }
        }
    });
    let token = config.api_token.clone();
    let api = Api {
        core: core.clone(),
        reader: Reader::open(&db).map_err(|e| format!("{}: {e}", db.display()))?,
        journal: Journal::new(&journal_dir),
        written: written.clone(),
        journal_health,
        stop: tokio::sync::watch::channel(None).0,
        daemon: json!({
            "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
            "socket": paths.socket, "started_at": aae_core::rfc3339_ms(Clock::system().now_ms()),
        }),
        config: RwLock::new(config),
        applied: tokio::sync::Mutex::new(1),
        load: Box::new(move || {
            let mut loaded = load(&paths.config).inspect(crate::doctor::warn_per_session);
            if let Ok(config) = &mut loaded {
                config.api_token.clone_from(&token);
            }
            loaded
        }),
        drivers,
    };
    let stop = stop_signal().map_err(|e| format!("signals: {e}"))?;
    eprintln!("aae daemon listening on {}", paths.socket.display());
    let served = api::serve(listener, tcp, api, stop).await;
    let _ = std::fs::remove_file(&paths.socket);
    // `daemon.stopping` goes to the journal before the exit; a drain emitted it when it began, so this repeat only
    // reads its `seq`. The writer is then left as a crash leaves it: the state is crash-safe.
    let reason = served.as_ref().map_or("signal", |r| r);
    let stopping = Msg::Stopping {
        reason: reason.to_owned(),
    };
    if let Ok(reply) = core.send(None, stopping).await {
        let seq = reply["seq"].as_i64().unwrap_or_default();
        let mut written = written;
        let flushed = written.wait_for(|w| *w >= seq);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), flushed).await;
    }
    drop((task, watchers));
    served
        .map(|reason| reason == "restart")
        .map_err(|e| format!("serving the API: {e}"))
}

/// Where the binary `name` is and who said so (spec §6.5): the config's `explicit` path, else mise (a daemon under
/// systemd or launchd has no mise shims on its `PATH`), else the bare name, looked up on `PATH` at each spawn.
pub(crate) fn binary(name: &str, explicit: Option<&Path>) -> (PathBuf, &'static str) {
    if let Some(path) = explicit {
        return (path.to_owned(), "config");
    }
    let mise = std::process::Command::new("mise").args(["which", name]).output();
    match mise {
        Ok(out) if out.status.success() => {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            (path.into(), "mise")
        }
        _ => (name.into(), "PATH"),
    }
}

/// The driver of harness `name` on its resolved binary, with the binary and its source; `None` for a harness aae
/// has no adapter for.
pub(crate) fn driver(
    name: &str,
    harness: &config::Harness,
    paths: &Paths,
) -> Option<(Arc<dyn Driver>, PathBuf, &'static str)> {
    let (binary, source) = binary(name, harness.binary.as_deref());
    let (state, socket) = (paths.state.clone(), Some(paths.socket.clone()));
    let driver: Arc<dyn Driver> = match name {
        "claude" => Arc::new(Claude::new(binary.clone(), state, socket)),
        "codex" => {
            let max_sessions = harness.max_sessions.map_or(8, |n| n.get() as usize);
            Arc::new(Codex::new(binary.clone(), state, socket, max_sessions))
        }
        _ => return None,
    };
    Some((driver, binary, source))
}

/// The harness versions the adapter of `driver` was tested on.
pub(crate) fn tested(driver: &Arc<dyn Driver>) -> [(u32, u32, u32); 2] {
    driver.capabilities(&ProcessPart(json!(null))).versions
}

/// The config in `file` with the parameters of the bundled plugins checked, or every error in it.
pub(crate) fn load(file: &Path) -> Result<config::Config, Vec<String>> {
    let config = config::load(file).map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>())?;
    let errors: Vec<String> = (config.bundled.iter())
        .filter_map(|b| aae_plugins::check(b.name, &b.params).err())
        .map(|e| e.to_string())
        .collect();
    match errors.is_empty() {
        true => Ok(config),
        false => Err(errors),
    }
}

/// An exclusive lock on the state directory; the OS drops it with the process, so a crash leaves none behind.
fn lock(state: &Path) -> Result<File, String> {
    let path = state.join("lock");
    let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(format!("another daemon owns {}", state.display())),
        Err(TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
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

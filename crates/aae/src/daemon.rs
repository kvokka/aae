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
    sync::Arc,
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

/// Runs the daemon until a stop signal; an error is why it could not start.
pub(crate) async fn run(paths: Paths) -> Result<(), String> {
    let config = config::load(&paths.config).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        format!("the config has errors:\n{}", lines.join("\n"))
    })?;
    private_dir(&paths.state)?;
    // Held until the daemon exits: one writer per state directory (spec §5.1), whatever socket it serves.
    let _lock = lock(&paths.state)?;
    let db = paths.state.join("state.db");
    let conn = open(&db).map_err(|e| format!("{}: {e}", db.display()))?;
    let journal_dir = paths.state.join("journal");
    let journal = Journal::new(&journal_dir);
    let written = journal.subscribe();
    let listener = bind(&paths.socket).await?;
    // Descendants of instances that left their group are re-parented here and reaped (invariant 7).
    aae_core::orphans::adopt_orphans().map_err(|e| format!("cannot become the child subreaper: {e}"))?;
    let mut drivers: Vec<Arc<dyn Driver>> = Vec::new();
    for (name, harness) in &config.harnesses {
        // shortcut: without `binary` the harness on PATH runs, not the mise-pinned one (§6.5); `aae doctor` (WP-35).
        let binary = harness.binary.clone().unwrap_or_else(|| name.into());
        let (state, socket) = (paths.state.clone(), Some(paths.socket.clone()));
        let driver: Arc<dyn Driver> = match name.as_str() {
            "claude" => Arc::new(Claude::new(binary.clone(), state, socket)),
            "codex" => Arc::new(Codex::new(binary.clone(), state, socket)),
            _ => continue,
        };
        let tested = driver.capabilities(&ProcessPart(json!(null))).versions;
        if let Some(why) = aae_core::untested(&binary, tested) {
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
    let trackers: BTreeMap<String, Arc<dyn Tracker>> = beads
        .iter()
        .map(|(name, dir)| (name.clone(), Arc::new(Br::new(dir.clone())) as Arc<dyn Tracker>))
        .collect();
    if let Some(why) = (!beads.is_empty())
        .then(|| aae_core::untested(Path::new("br"), aae_br::TESTED))
        .flatten()
    {
        eprintln!("aae daemon: warning: {why}");
    }
    let config = Arc::new(config);
    let (core, task) = run::start(
        conn,
        Clock::system(),
        config.clone(),
        drivers,
        trackers,
        Arc::new(ExecHooks {
            socket: Some(paths.socket.clone()),
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
    let api = Api {
        core,
        reader: Reader::open(&db).map_err(|e| format!("{}: {e}", db.display()))?,
        journal: Journal::new(&journal_dir),
        written,
        daemon: json!({
            "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
            "socket": paths.socket, "started_at": aae_core::rfc3339_ms(Clock::system().now_ms()),
        }),
        config,
    };
    let stop = stop_signal().map_err(|e| format!("signals: {e}"))?;
    eprintln!("aae daemon listening on {}", paths.socket.display());
    let served = api::serve(listener, api, stop).await;
    let _ = std::fs::remove_file(&paths.socket);
    // shortcut: the writer is left as a crash leaves it (the state is crash-safe); a drain that lets running
    // attempts end first comes with the daemon operations of WP-22b.
    drop((task, watchers));
    served.map_err(|e| format!("serving the API: {e}"))
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

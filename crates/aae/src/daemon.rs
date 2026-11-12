//! `aae daemon`: the core and the API on a unix socket, in the foreground (spec §13), until SIGTERM or SIGINT.

use aae_core::{
    Clock, Journal, config, open,
    run::{self, ExecHooks, Reader},
};
use aae_front::api::{self, Api};
use serde_json::json;
use std::{
    fs::DirBuilder,
    io,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    net::{UnixListener, UnixStream},
    signal::unix::{SignalKind, signal},
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
        let home = || std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set");
        let xdg = |var: &str, fallback: &str| -> Result<PathBuf, String> {
            match std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()) {
                Some(dir) => Ok(dir),
                None => Ok(home()?.join(fallback)),
            }
        };
        let state = xdg("XDG_STATE_HOME", ".local/state")?.join("aae");
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map_or_else(|| state.clone(), |dir| dir.join("aae"));
        let config = match config {
            Some(config) => config,
            None => xdg("XDG_CONFIG_HOME", ".config")?.join("aae/config.toml"),
        };
        Ok(Self {
            config,
            state,
            socket: socket.unwrap_or_else(|| runtime.join("aae.sock")),
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
    let db = paths.state.join("state.db");
    let conn = open(&db).map_err(|e| format!("{}: {e}", db.display()))?;
    let journal_dir = paths.state.join("journal");
    let journal = Journal::new(&journal_dir);
    let written = journal.subscribe();
    let listener = bind(&paths.socket).await?;
    let (core, task) = run::start(
        conn,
        Clock::system(),
        Arc::new(config),
        Vec::new(),
        Arc::new(ExecHooks),
        journal,
    );
    let api = Api {
        core,
        reader: Reader::open(&db).map_err(|e| format!("{}: {e}", db.display()))?,
        journal: Journal::new(&journal_dir),
        written,
        daemon: json!({
            "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
            "socket": paths.socket, "started_at": aae_core::rfc3339_ms(Clock::system().now_ms()),
        }),
    };
    let stop = stop_signal().map_err(|e| format!("signals: {e}"))?;
    eprintln!("aae daemon listening on {}", paths.socket.display());
    let served = api::serve(listener, api, stop).await;
    let _ = std::fs::remove_file(&paths.socket);
    // shortcut: the writer is left as a crash leaves it (the state is crash-safe); a drain that lets running
    // attempts end first comes with the daemon operations of WP-22b.
    drop(task);
    served.map_err(|e| format!("serving the API: {e}"))
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
    UnixListener::bind(socket).map_err(|e| format!("{shown}: {e}"))
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

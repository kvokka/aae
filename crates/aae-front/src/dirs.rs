//! Where the daemon keeps its files, by the XDG base directories (docs/guide/install.md); the CLI finds the socket the
//! same way.

use std::path::PathBuf;

/// `$<var>` if it is an absolute path, else `$HOME/<fallback>`.
pub fn xdg(var: &str, fallback: &str) -> Result<PathBuf, String> {
    let set = std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute());
    match set {
        Some(dir) => Ok(dir),
        None => Ok(PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join(fallback)),
    }
}

/// The daemon's state: `state.db` and `journal/`.
pub fn state() -> Result<PathBuf, String> {
    Ok(xdg("XDG_STATE_HOME", ".local/state")?.join("aae"))
}

/// The packs and addons downloaded from git.
pub fn cache() -> Result<PathBuf, String> {
    Ok(xdg("XDG_CACHE_HOME", ".cache")?.join("aae/git"))
}

/// The API socket: in `$XDG_RUNTIME_DIR/aae`, or in the state directory without one.
pub fn socket() -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    match runtime {
        Some(dir) => Ok(dir.join("aae/aae.sock")),
        None => Ok(state()?.join("aae.sock")),
    }
}

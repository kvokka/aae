//! Packs and addons from git (docs/design/config.md, docs/guide/addons.md): downloaded into the user's cache before
//! each load of the config, so the core reads only finished directories (docs/guide/install.md). A commit or a tag in
//! the cache needs no network; a source without a pin asks the remote for its latest revision at every load.
//!
//! The cache: `<cache>/<key(url)>/<commit>/` is a detached checkout, never changed once in place;
//! `<cache>/<key(url)>/tag-<key(tag)>` and `<cache>/<key(url)>/head` name the commit a tag and the latest revision
//! were last seen at.

use loop_manager_core::config::{Checkouts, GitSource, commit_id, git_sources};
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    sync::{Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// How long one git command may take before it is killed: a load of the config must not wait on the network forever.
const DEADLINE: Duration = Duration::from_secs(30);

/// Where each git source of the config in `config` is in `cache`, downloaded if it is not there yet.
pub(crate) fn checkouts(cache: &Path, config: &Path) -> Checkouts {
    // Reload, check and doctor of one daemon may load at once; a clone's temporary directory is named by the pid.
    static LOCK: Mutex<()> = Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let at = |s: GitSource| (checkout(cache, &s), s);
    git_sources(config)
        .into_iter()
        .map(at)
        .map(|(dir, s)| (s, dir))
        .collect()
}

/// The checkout of `source` in `cache`, downloaded when the cache does not have it.
fn checkout(cache: &Path, source: &GitSource) -> Result<PathBuf, String> {
    let repo = cache.join(key(&source.git));
    // What to resolve in a clone, what it is when it is not there, and the file that names the commit found.
    let (want, missing, pointer) = match &source.rev {
        Some(commit) if commit_id(commit) => match repo.join(commit) {
            dir if dir.is_dir() => return Ok(dir),
            _ => (commit.clone(), format!("no commit `{commit}` in the repository"), None),
        },
        Some(tag) => {
            let pointer = repo.join(format!("tag-{}", key(tag)));
            // A tag does not move: one in the cache is not asked again.
            if let Some(dir) = cached(&repo, &pointer) {
                return Ok(dir);
            }
            let missing = format!("no tag `{tag}` in the repository; `rev` is a tag or a full commit id");
            (format!("refs/tags/{tag}"), missing, Some(pointer))
        }
        None => {
            let pointer = repo.join("head");
            match latest(&source.git) {
                Ok(commit) if repo.join(&commit).is_dir() => {
                    write(&pointer, &commit)?;
                    return Ok(repo.join(commit));
                }
                Ok(_) => (
                    "HEAD".to_owned(),
                    "the repository has no commits".to_owned(),
                    Some(pointer),
                ),
                Err(e) => {
                    let Some(dir) = cached(&repo, &pointer) else {
                        return Err(e);
                    };
                    let commit = dir.file_name().unwrap_or_default().to_string_lossy();
                    eprintln!(
                        "loop-manager: warning: `{}`: the latest revision is not known ({e}); using `{commit}` from the cache",
                        source.git
                    );
                    return Ok(dir);
                }
            }
        }
    };
    fs::create_dir_all(&repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    let temp = repo.join(format!(".clone-{}", std::process::id()));
    let placed = clone(&temp, &source.git, &want, missing).and_then(|commit| {
        let dir = repo.join(&commit);
        match fs::rename(&temp, &dir) {
            Ok(()) => Ok((commit, dir)),
            // Another process placed the same commit first.
            Err(_) if dir.is_dir() => Ok((commit, dir)),
            Err(e) => Err(format!("{}: {e}", dir.display())),
        }
    });
    if temp.exists() {
        let _ = fs::remove_dir_all(&temp);
    }
    let (commit, dir) = placed?;
    pointer.map_or(Ok(()), |p| write(&p, &commit))?;
    Ok(dir)
}

/// The checkout the commit in `pointer` names, when both are there.
fn cached(repo: &Path, pointer: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(pointer).ok()?;
    let commit = text.trim();
    Some(repo.join(commit)).filter(|dir| commit_id(commit) && dir.is_dir())
}

fn write(pointer: &Path, commit: &str) -> Result<(), String> {
    fs::write(pointer, format!("{commit}\n")).map_err(|e| format!("{}: {e}", pointer.display()))
}

/// The commit `HEAD` of the remote `url` is at now.
fn latest(url: &str) -> Result<String, String> {
    let out = git(&["ls-remote", "--", url, "HEAD"])?;
    let commit = out.split_whitespace().next().unwrap_or_default();
    match commit_id(commit) {
        true => Ok(commit.to_owned()),
        false => Err("the repository has no commits".to_owned()),
    }
}

/// Clones `url` into `temp` and checks out the commit `want` names there; `missing` is the error when none.
fn clone(temp: &Path, url: &str, want: &str, missing: String) -> Result<String, String> {
    if temp.exists() {
        fs::remove_dir_all(temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    }
    let path = temp.to_string_lossy();
    git(&["clone", "--quiet", "--no-checkout", "--", url, &path])?;
    let resolve = format!("{want}^{{commit}}");
    let args = [
        "-C",
        &path,
        "rev-parse",
        "--verify",
        "--quiet",
        "--end-of-options",
        &resolve,
    ];
    let commit = git(&args).map_err(|_| missing)?;
    if !commit_id(&commit) {
        return Err(format!("`{commit}` is not a full commit id"));
    }
    git(&["-C", &path, "checkout", "--quiet", "--detach", &commit])?;
    match git(&["-C", &path, "rev-parse", "HEAD"])? {
        head if head == commit => Ok(commit),
        head => Err(format!("checked out {head}, not {commit}")),
    }
}

/// Runs git without a terminal to ask for a password on, killed past [`DEADLINE`]; its trimmed output.
fn git(args: &[&str]) -> Result<String, String> {
    // Dropped once the child was waited for, on every way out.
    let (mut child, _own) = loop_manager_core::orphans::spawn_std(
        Command::new("git")
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .map_err(|e| format!("git: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut text = String::new();
            pipe.map(|mut p| p.read_to_string(&mut text));
            text
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let until = Instant::now() + DEADLINE;
    let status = loop {
        match child.try_wait().map_err(|e| format!("git: {e}"))? {
            Some(status) => break status,
            None if Instant::now() >= until => {
                // The readers are left behind: a helper of git (ssh, a remote helper) may hold the pipes a while.
                let _ = child.kill();
                let _ = child.wait();
                let secs = DEADLINE.as_secs();
                return Err(format!("git {}: no answer in {secs}s", args.join(" ")));
            }
            None => thread::sleep(Duration::from_millis(20)),
        }
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    match status.success() {
        true => Ok(out.trim().to_owned()),
        false => Err(format!("git {}: {}", args.join(" "), err.trim())),
    }
}

/// `text` as one file name: `[A-Za-z0-9_-]`, and `.` past the first byte, as they are; any other byte `%XX`. Neither
/// `..` nor `/` gets through.
fn key(text: &str) -> String {
    let kept = |i: usize, b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || (b == b'.' && i > 0);
    let bytes = text.bytes().enumerate();
    bytes
        .map(|(i, b)| {
            if kept(i, b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `loop-manager cache clear`: removes every downloaded pack and addon; none there is cleared too.
pub(crate) fn clear(cache: &Path, json: bool) -> ExitCode {
    match fs::remove_dir_all(cache) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return loop_manager_front::cli::fail(json, "io_error", &format!("{}: {e}", cache.display())),
    }
    loop_manager_front::cli::print(json, &serde_json::json!({ "cleared": cache.to_string_lossy() }));
    ExitCode::SUCCESS
}

#[cfg(test)]
#[test]
fn a_key_is_one_safe_file_name() {
    assert_eq!(key("https://x.org/a.git"), "https%3A%2F%2Fx.org%2Fa.git");
    assert_eq!(key("../v1"), "%2E.%2Fv1");
}

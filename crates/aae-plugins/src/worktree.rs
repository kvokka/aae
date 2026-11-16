//! `worktree` (docs/design/config.md): on `run.preparing` a run of a formula with `worktree = true` gets its own git
//! worktree on the branch `aae/<run>` as its workdir; once the run ends the worktree goes, only when it has no
//! uncommitted change and its commits are reachable from a ref. The branch stays unless it is merged. A run continuing
//! the session of a run that had a worktree works in that same worktree, recreated on its branch if it was removed.

use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{File, TryLockError},
    io::Read,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Set by the loader, not by a table: the qualified names of the formulas with `worktree = true`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Params {
    formulas: Vec<String>,
}

/// Where the worktrees of a repository live: inside its git directory, out of every working tree.
const DIR: &str = "aae/worktrees";

/// The timeout of a bundled hook (contracts 3.1): past it the runner gives up, and the `git` still running is killed
/// with its process group rather than left to run on.
const TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 30 });

pub(crate) fn hook(event: &Value, p: &Params) -> Result<Option<Value>, String> {
    let run = &event["data"]["run"];
    let id = run["id"].as_str().ok_or("the event has no run")?;
    let workdir = Path::new(run["workdir"].as_str().ok_or("the run has no workdir")?);
    let until = Instant::now() + TIMEOUT;
    match (event["type"].as_str(), worktree(workdir)) {
        // A run continuing another's session got that run's workdir: its worktree, brought back if removed.
        (Some("run.preparing"), Some((common, of))) => create(&common, of, until).map(Some),
        (Some("run.preparing"), None) if p.formulas.iter().any(|f| run["formula"] == f.as_str()) => {
            // A project root may be a plain directory (docs/design/config.md): the run fails, not the hook.
            if !workdir.ancestors().any(|d| d.join(".git").exists()) {
                let reason = format!(
                    "worktree: `{}` is not a git repository; the formula has `worktree = true`",
                    workdir.display()
                );
                return Ok(Some(json!({ "decision": "deny", "reason": reason })));
            }
            let common = git(
                workdir,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                until,
            )?;
            create(Path::new(&common), id, until).map(Some)
        }
        (Some("run.passed" | "run.failed" | "run.cancelled"), Some((common, of))) => {
            remove(&common, workdir, of, until).map(|()| None)
        }
        _ => Ok(None),
    }
}

/// The git common dir and the run of a worktree this plugin made: `<common>/aae/worktrees/<run>`.
fn worktree(workdir: &Path) -> Option<(PathBuf, &str)> {
    let of = workdir.file_name()?.to_str()?;
    let dir = workdir.parent()?;
    let common = dir.ancestors().nth(2)?;
    dir.ends_with(DIR).then(|| (common.to_owned(), of))
}

/// Creates the worktree of run `id` in the repository with git dir `common`, from its HEAD; one already there is
/// reused, so a hook run again after a crash decides the same; an existing branch `aae/<id>` is checked out, never
/// reset.
fn create(common: &Path, id: &str, until: Instant) -> Result<Value, String> {
    let _held = hold(common, until)?;
    let path = common.join(DIR).join(id);
    if !path.join(".git").exists() {
        let branch = format!("aae/{id}");
        let at = path.to_string_lossy();
        // A worktree removed by hand stays registered, and its branch may hold commits: take the branch as it is.
        git(common, &["worktree", "prune"], until)?;
        if git(common, &["branch", "--list", &branch], until)?.is_empty() {
            git(common, &["worktree", "add", "-b", &branch, &at, "HEAD"], until)?;
        } else {
            git(common, &["worktree", "add", &at, &branch], until)?;
        }
    }
    Ok(json!({ "decision": "proceed", "workdir": path }))
}

/// Removes the worktree at `path` of run `id` unless it holds work a removal would lose.
fn remove(common: &Path, path: &Path, id: &str, until: Instant) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let _held = hold(common, until)?;
    let head = git(path, &["rev-parse", "HEAD"], until)?;
    if git(
        path,
        &["for-each-ref", "--contains", &head, "--format=%(refname)"],
        until,
    )?
    .is_empty()
    {
        return Err(format!(
            "{} keeps commits no ref reaches; left in place",
            path.display()
        ));
    }
    // Refuses a worktree with uncommitted changes.
    git(common, &["worktree", "remove", &path.to_string_lossy()], until)?;
    // Deletes the branch only when it is merged; an unmerged one is the run's work.
    let _ = git(common, &["branch", "-d", &format!("aae/{id}")], until);
    Ok(())
}

/// Locks the git dir `common` itself, so the runs of one repository change its worktrees one at a time: two
/// `git worktree add` at once race on the repository's config, which the first one may write
/// (`worktree.useRelativePaths` sets `extensions.relativeWorktrees`). The lock goes with the returned file.
fn hold(common: &Path, until: Instant) -> Result<File, String> {
    let dir = File::open(common).map_err(|e| format!("{}: {e}", common.display()))?;
    loop {
        match dir.try_lock() {
            Ok(()) => return Ok(dir),
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(format!("{}: locked past the hook timeout", common.display()));
            }
            Err(TryLockError::Error(e)) => return Err(format!("{}: {e}", common.display())),
        }
    }
}

/// Runs `git` in its own process group, killed with the group once `until` passes.
fn git(dir: &Path, args: &[&str], until: Instant) -> Result<String, String> {
    // Dropped once the child was waited for, on every way out.
    let (mut child, _own) = aae_core::orphans::spawn_std(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0),
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
    let status = loop {
        match child.try_wait().map_err(|e| format!("git: {e}"))? {
            Some(status) => break Some(status),
            None if Instant::now() >= until => break None,
            None => thread::sleep(Duration::from_millis(20)),
        }
    };
    let Some(status) = status else {
        let group = rustix::process::Pid::from_child(&child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        let _ = child.wait();
        return Err(format!("git {}: killed past the hook timeout", args.join(" ")));
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if !status.success() {
        return Err(format!("git {}: {}", args.join(" "), err.trim()));
    }
    Ok(out.trim().to_owned())
}

//! `aae fetch`: pins the git packs and plugins of the config to commits in `aae.lock` and checks them out
//! (spec §8.2, §11.6). A pin moves only with `--update`; the daemon never fetches.

use aae_core::config::{GitSource, Lock, Pinned, checkout, commit_id, git_sources};
use std::{
    fs,
    path::Path,
    process::{Command, ExitCode},
};

pub(crate) fn run(config: &Path, update: bool) -> ExitCode {
    let declared = match git_sources(config) {
        Ok(declared) => declared,
        Err(errors) => {
            errors.iter().for_each(|e| eprintln!("aae fetch: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for (dir, sources) in declared {
        let old = match Lock::read(&dir) {
            Ok(lock) => lock,
            Err(e) => {
                eprintln!("aae fetch: {e}");
                failed = true;
                continue;
            }
        };
        // Pins of sources no longer declared are dropped; a source that fails keeps its old pin.
        let mut lock = Lock::default();
        for source in sources {
            let pinned = old.commit(&source);
            let commit = match pin(&dir, &source, pinned, update) {
                Ok((commit, how)) => {
                    println!("{} {} -> {commit} ({how})", source.git, source.rev);
                    commit
                }
                Err(e) => {
                    eprintln!("aae fetch: {} {}: {e}", source.git, source.rev);
                    failed = true;
                    let Some(commit) = pinned else { continue };
                    commit.to_owned()
                }
            };
            lock.source.push(Pinned {
                git: source.git,
                rev: source.rev,
                commit,
            });
        }
        if !(lock.source.is_empty() && old.source.is_empty())
            && let Err(e) = lock.write(&dir)
        {
            eprintln!("aae fetch: {e}");
            failed = true;
        }
    }
    match failed {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    }
}

/// The commit `source` is pinned to, checked out in `dir`, and whether it was fetched, kept or updated.
fn pin(dir: &Path, source: &GitSource, pinned: Option<&str>, update: bool) -> Result<(String, &'static str), String> {
    if let Some(commit) = pinned.filter(|c| !update && checkout(dir, c).is_dir()) {
        return Ok((commit.to_owned(), "kept"));
    }
    let root = dir.join(".aae/git");
    fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let temp = root.join(format!(".clone-{}", std::process::id()));
    let result = clone(&temp, source, pinned.filter(|_| !update));
    let placed = result.as_ref().is_ok_and(|commit| !checkout(dir, commit).exists());
    let moved = match &result {
        Ok(commit) if placed => fs::rename(&temp, checkout(dir, commit)).map_err(|e| e.to_string()),
        _ => Ok(()),
    };
    // The clone is not needed when it failed or the commit was already checked out.
    if temp.exists() {
        let _ = fs::remove_dir_all(&temp);
    }
    let commit = result?;
    moved?;
    let how = match pinned {
        Some(old) if old != commit => "updated",
        Some(_) if !placed => "kept",
        _ => "fetched",
    };
    Ok((commit, how))
}

/// Clones `source` into `temp` and checks out `commit`, or the commit its revision names now.
fn clone(temp: &Path, source: &GitSource, commit: Option<&str>) -> Result<String, String> {
    if temp.exists() {
        fs::remove_dir_all(temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    }
    let path = temp.to_string_lossy();
    git(&["clone", "--quiet", "--no-checkout", "--", &source.git, &path])?;
    let commit = match commit {
        Some(commit) => commit.to_owned(),
        // A branch is `origin/<rev>` in the clone; a tag or a commit is itself.
        None => [format!("origin/{}", source.rev), source.rev.clone()]
            .iter()
            .find_map(|rev| {
                git(&[
                    "-C",
                    &path,
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "--end-of-options",
                    &format!("{rev}^{{commit}}"),
                ])
                .ok()
            })
            .ok_or_else(|| format!("no commit `{}` in the repository", source.rev))?,
    };
    if !commit_id(&commit) {
        return Err(format!("`{commit}` is not a full commit id"));
    }
    git(&["-C", &path, "checkout", "--quiet", "--detach", &commit])?;
    match git(&["-C", &path, "rev-parse", "HEAD"])? {
        head if head == commit => Ok(commit),
        head => Err(format!("checked out {head}, not {commit}")),
    }
}

/// Runs git and returns its trimmed output.
fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    match out.status.success() {
        true => Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned()),
        false => Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

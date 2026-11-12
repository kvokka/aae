//! `worktree` (spec §11.5): on `run.preparing` a run of a chosen agent or formula gets its own git worktree on the
//! branch `aae/<run>` as its workdir; once the run ends the worktree goes, only when it has no uncommitted change and
//! its commits are reachable from a ref. The branch stays unless it is merged.

use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// The `[builtin.worktree]` table: the agents and formulas whose runs get a worktree, by name or qualified name;
/// both empty means every run.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Params {
    agents: Vec<String>,
    formulas: Vec<String>,
}

/// Where the worktrees of a repository live: inside its git directory, out of every working tree.
const DIR: &str = "aae/worktrees";

pub(crate) fn hook(event: &Value, p: &Params) -> Result<Option<Value>, String> {
    let run = &event["data"]["run"];
    let id = run["id"].as_str().ok_or("the event has no run")?;
    let workdir = Path::new(run["workdir"].as_str().ok_or("the run has no workdir")?);
    match event["type"].as_str() {
        Some("run.preparing") if chosen(run, p) => create(workdir, id).map(Some),
        Some("run.passed" | "run.failed" | "run.cancelled") if ours(workdir, id) => remove(workdir, id).map(|()| None),
        _ => Ok(None),
    }
}

fn chosen(run: &Value, p: &Params) -> bool {
    let named = |list: &[String], name: &Value| {
        let name = name.as_str().unwrap_or_default();
        let bare = name.rsplit('.').next().unwrap_or(name);
        list.iter().any(|n| n == name || n == bare)
    };
    (p.agents.is_empty() && p.formulas.is_empty())
        || named(&p.agents, &run["agent"])
        || named(&p.formulas, &run["formula"])
}

fn ours(workdir: &Path, id: &str) -> bool {
    workdir.ends_with(Path::new(DIR).join(id))
}

/// Creates the worktree of run `id` from the HEAD of the repository at `workdir`; one already there is reused, so a
/// hook run again after a crash decides the same.
fn create(workdir: &Path, id: &str) -> Result<Value, String> {
    let common = git(workdir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let path = PathBuf::from(common).join(DIR).join(id);
    if !path.join(".git").exists() {
        let branch = format!("aae/{id}");
        let at = path.to_string_lossy();
        git(workdir, &["worktree", "add", "-B", &branch, &at, "HEAD"])?;
    }
    Ok(json!({ "decision": "proceed", "workdir": path }))
}

/// Removes the worktree at `path` unless it holds work a removal would lose.
fn remove(path: &Path, id: &str) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let head = git(path, &["rev-parse", "HEAD"])?;
    if git(path, &["for-each-ref", "--contains", &head, "--format=%(refname)"])?.is_empty() {
        return Err(format!(
            "{} keeps commits no ref reaches; left in place",
            path.display()
        ));
    }
    let common = git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    // Refuses a worktree with uncommitted changes.
    git(Path::new(&common), &["worktree", "remove", &path.to_string_lossy()])?;
    // Deletes the branch only when it is merged; an unmerged one is the run's work.
    let _ = git(Path::new(&common), &["branch", "-d", &format!("aae/{id}")]);
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {}: {}", args.join(" "), err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

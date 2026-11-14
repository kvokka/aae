//! The bundled `worktree` plugin through a real `aae daemon` and the real `claude` on the fake Anthropic API
//! (spec §11.5): the session of a formula with `worktree = true` works in its own git worktree, which goes once the run passes.

mod common;

use aae_testkit::anthropic::{Provider, Reply};
use common::Daemon;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// `git -C <dir> <args>` without the user's git config; its trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_session_of_a_worktree_formula_works_in_its_worktree() {
    let out = tempfile::tempdir().unwrap();
    let cwd = out.path().join("cwd");
    let finish = format!(
        "pwd > {} && {} finish pass --summary built",
        cwd.display(),
        env!("CARGO_BIN_EXE_aae")
    );
    let provider = Provider::start(vec![Reply::Bash(finish), Reply::Text("Done.".into())]).await;
    let base_url = provider.base_url();
    let d = Daemon::start(|dir| {
        let shop = dir.join("config/aae/shop");
        let files = [
            (
                dir.join("config/aae/config.toml"),
                "projects = [\"shop\"]\n[harness.claude]\n".to_owned(),
            ),
            (
                shop.join("agents/dev/agent.toml"),
                format!(
                    "harness = \"claude\"\nmodel = \"claude-haiku-5-5\"\npermission_mode = \"bypassPermissions\"\n\
                     inherit_host = false\n[env]\nANTHROPIC_BASE_URL = \"{base_url}\"\nANTHROPIC_API_KEY = \"fake\"\n\
                     CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n"
                ),
            ),
            (
                shop.join("formulas/build.toml"),
                "name = \"build\"\nworktree = true\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n"
                    .to_owned(),
            ),
            (shop.join("formulas/build.md"), "Build {{ rig.name }}.".to_owned()),
        ];
        for (path, text) in files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let rig = shop.join("web");
        git(&rig, &["init", "-q", "-b", "main"]);
        git(&rig, &["add", "-A"]);
        git(&rig, &["commit", "-q", "-m", "base"]);
    })
    .await;
    let rig = d.dir.path().join("config/aae/shop/web");

    let run = d.create("build", None).await;
    let view = d.until_state(&run, "passed").await;
    let tree = PathBuf::from(view["workdir"].as_str().unwrap());
    assert!(tree.ends_with(format!("aae/worktrees/{run}")), "{tree:?}");
    assert_eq!(
        fs::read_to_string(&cwd).unwrap().trim(),
        tree.to_str().unwrap(),
        "the session's cwd"
    );
    // No commit and no change: the worktree goes, and its merged branch with it (two commands, so wait for both).
    let branch = format!("aae/{run}");
    let deadline = Instant::now() + Duration::from_secs(30);
    while tree.exists() || !git(&rig, &["branch", "--list", &branch]).is_empty() {
        assert!(Instant::now() < deadline, "{tree:?} or {branch} stays");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

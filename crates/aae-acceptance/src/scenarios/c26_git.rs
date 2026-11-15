//! Criterion 26: packs from git by tag, by commit and unpinned. The daemon downloads them into the cache itself; a
//! cached tag or commit needs no network, an unpinned pack follows the latest revision and falls back to the cache
//! with a warning; `aae cache clear` makes the next load download again.

use crate::{
    Env,
    daemon::{Daemon, write},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, sync::Arc};

/// Runs git in `repo` away from the user's git config.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Commits a change in `repo` and returns the commit.
fn commit(repo: &Path, text: &str) -> String {
    write(repo.join("README"), text);
    git(repo, &["add", "."]);
    git(repo, &["commit", "--quiet", "-m", text]);
    git(repo, &["rev-parse", "HEAD"])
}

/// The repository `remote/<name>` with pack `name` in its first commit, returned.
fn repo(root: &Path, name: &str) -> String {
    let repo = root.join("remote").join(name);
    write(
        repo.join("pack.toml"),
        format!("name = \"{name}\"\nversion = \"0.1.0\"\n"),
    );
    let formula = "name = \"one\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";
    write(repo.join("formulas/one.toml"), formula);
    git(&repo, &["init", "--quiet", "-b", "main"]);
    commit(&repo, "A")
}

/// The root of each pack from git, by name.
async fn roots(d: &Daemon) -> [String; 3] {
    let packs = d.get("/v1/packs?project=shop").await;
    let root = |name: &str| {
        let list = packs.as_array().expect("a list of packs");
        let pack = list.iter().find(|p| p["name"] == name);
        pack.and_then(|p| p["root"].as_str())
            .unwrap_or_else(|| panic!("no pack {name}: {packs}"))
            .to_owned()
    };
    ["tagged", "pinned", "latest"].map(root)
}

async fn reload(d: &Daemon) -> Result<Value, String> {
    d.post("/v1/config/reload", json!({}))
        .await
        .map_err(|e| format!("{e:?}"))
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let mut first = Vec::new();
    let d = Daemon::start(&env, |root| {
        let tagged = repo(root, "tagged");
        git(&root.join("remote/tagged"), &["tag", "v1"]);
        commit(&root.join("remote/tagged"), "B");
        let pinned = repo(root, "pinned");
        commit(&root.join("remote/pinned"), "B");
        let latest = repo(root, "latest");
        let url = |name: &str| root.join("remote").join(name).display().to_string();
        let packs = format!(
            "packs = [{{ git = \"{}\", rev = \"v1\" }}, {{ git = \"{}\", rev = \"{pinned}\" }}, {{ git = \"{}\" }}]\n",
            url("tagged"),
            url("pinned"),
            url("latest")
        );
        write(
            root.join("config/aae/shop/aae.toml"),
            format!("{packs}[rigs.web]\npath = \"web\"\n"),
        );
        first = vec![tagged, pinned, latest];
    })
    .await;
    let cache = d.path("cache/aae/git");
    let roots_now = roots(&d).await;
    for (root, commit) in roots_now.iter().zip(&first) {
        assert!(root.ends_with(commit.as_str()), "{root} is not at {commit}");
        assert!(
            Path::new(root).starts_with(&cache) && Path::new(root).is_dir(),
            "{root}"
        );
    }

    let newer = commit(&d.path("remote/latest"), "C");
    reload(&d).await.expect("a reload");
    let [tagged, pinned, latest] = roots(&d).await;
    assert!(latest.ends_with(&newer), "the latest revision is {newer}: {latest}");

    fs::rename(d.path("remote"), d.path("remote.off")).unwrap();
    reload(&d).await.expect("the cache serves without the remotes");
    assert_eq!(roots(&d).await, [tagged, pinned, latest]);
    let log = fs::read_to_string(d.path("daemon.log")).unwrap();
    let said = |name: &str| log.lines().any(|l| l.contains(&format!("remote/{name}")));
    assert!(
        log.lines()
            .any(|l| l.contains("warning") && l.contains("remote/latest")),
        "{log}"
    );
    assert!(
        !said("tagged") && !said("pinned"),
        "a tag or a commit in the cache is not asked again: {log}"
    );

    let out = d.cli(&["cache", "clear"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!cache.exists());
    assert!(reload(&d).await.is_err(), "nothing to load the packs from");

    fs::rename(d.path("remote.off"), d.path("remote")).unwrap();
    reload(&d).await.expect("downloaded again");
    let again = roots(&d).await;
    assert!(again.iter().all(|r| Path::new(r).is_dir()), "{again:?}");
    "a tag, a commit and the latest revision downloaded by the daemon; the cache serves offline; clear downloads again"
        .into()
}

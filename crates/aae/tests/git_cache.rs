//! Packs from git on local repositories: downloaded into the cache at a tag, a commit or the latest revision, kept
//! there without the network, removed by `aae cache clear` (docs/design/config.md, docs/guide/plugin.md).

use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    process::{Command, Output},
};

/// Runs git or `aae` away from the user's git config, whose hooks would run on the commits here.
fn isolated(mut command: Command, dir: &Path) -> Output {
    command
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn git(dir: &Path, repo: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(repo)
        .args(args);
    let out = isolated(command, dir);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A repository on branch `main` with `files` committed.
fn repo(dir: &Path, name: &str, files: &[(&str, &str)]) -> String {
    let repo = dir.join("remote").join(name);
    fs::create_dir_all(&repo).unwrap();
    git(dir, &repo, &["init", "--quiet", "-b", "main"]);
    commit(dir, &repo, files)
}

/// Commits `files` in `repo` and returns the commit.
fn commit(dir: &Path, repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
        fs::write(repo.join(path), text).unwrap();
    }
    git(dir, repo, &["add", "."]);
    git(dir, repo, &["commit", "--quiet", "-m", "c"]);
    git(dir, repo, &["rev-parse", "HEAD"])
}

/// `aae` with `args`: whether it succeeded, its stdout and its stderr.
fn aae(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
    command.args(args);
    let out = isolated(command, dir);
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (out.status.success(), text(&out.stdout), text(&out.stderr))
}

const PACK: &[(&str, &str)] = &[
    ("pack.toml", "name = \"remote\"\nversion = \"0.1.0\"\n"),
    (
        "formulas/check.toml",
        "name = \"check\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
    ),
];

/// The project `shop` with the packs `packs`: `(repository, rev)`.
fn config(dir: &Path, packs: &[(&str, Option<&str>)]) {
    let shop = dir.join("config/aae/shop");
    fs::create_dir_all(&shop).unwrap();
    fs::write(dir.join("config/aae/config.toml"), "projects = [\"shop\"]\n").unwrap();
    let pack = |(name, rev): &(&str, Option<&str>)| {
        let rev = rev.map(|r| format!(", rev = \"{r}\"")).unwrap_or_default();
        format!("{{ git = \"{}\"{rev} }}", dir.join("remote").join(name).display())
    };
    let packs: Vec<String> = packs.iter().map(pack).collect();
    fs::write(shop.join("aae.toml"), format!("packs = [{}]\n", packs.join(", "))).unwrap();
}

/// The directories in the cache: `(repository, commit or temporary)`.
fn cached(dir: &Path) -> BTreeSet<String> {
    let entries = |d: &Path| fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path());
    let repos = entries(&dir.join("cache/aae/git"));
    let all = repos.flat_map(|r| entries(&r).filter(|p| p.is_dir()).collect::<Vec<_>>());
    all.map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// `aae config check --json`: whether the config loaded, and stdout with stderr.
fn check(dir: &Path) -> (bool, String) {
    let (ok, out, err) = aae(dir, &["config", "check", "--json"]);
    (ok, format!("{out}{err}"))
}

#[test]
fn a_rev_that_is_no_tag_and_no_commit_is_a_config_error() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    repo(d, "pack", PACK);
    let missing = "0123456789abcdef0123456789abcdef01234567";
    for (name, rev) in [("pack", Some("main")), ("pack", Some(missing)), ("away", None)] {
        config(d, &[(name, rev)]);
        let (ok, out) = check(d);
        assert!(!ok, "{name} {rev:?}: {out}");
        let url = d.join("remote").join(name);
        assert!(
            out.contains("aae.toml") && out.contains(&*url.to_string_lossy()),
            "{out}"
        );
    }
    assert!(cached(d).iter().all(|c| !c.starts_with(".clone-")), "{:?}", cached(d));
}

#[test]
fn a_tag_in_the_cache_is_not_asked_again() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let first = repo(d, "pack", PACK);
    let remote = d.join("remote/pack");
    git(d, &remote, &["tag", "v1"]);
    config(d, &[("pack", Some("v1"))]);
    let (ok, out) = check(d);
    assert!(ok, "{out}");

    commit(d, &remote, &[("README", "moved")]);
    git(d, &remote, &["tag", "-f", "v1"]);
    fs::rename(d.join("remote"), d.join("remote.off")).unwrap();
    let (ok, out) = check(d);
    assert!(ok, "the tag is in the cache: {out}");
    assert_eq!(cached(d), [first].into(), "a moved tag is not seen");
}

#[test]
fn without_a_pin_the_cache_serves_when_the_remote_is_away() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let first = repo(d, "pack", PACK);
    config(d, &[("pack", None)]);
    assert!(check(d).0);
    let second = commit(d, &d.join("remote/pack"), &[("README", "later")]);
    assert!(check(d).0);
    assert_eq!(
        cached(d),
        [first, second].into(),
        "every load asks for the latest revision"
    );

    fs::rename(d.join("remote"), d.join("remote.off")).unwrap();
    let (ok, out) = check(d);
    assert!(ok, "{out}");
    let url = d.join("remote/pack");
    assert!(
        out.contains("warning") && out.contains(&*url.to_string_lossy()),
        "{out}"
    );

    assert!(aae(d, &["cache", "clear"]).0);
    assert!(!check(d).0, "nothing to load from");
}

#[test]
fn cache_clear_needs_no_daemon_and_no_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    fs::create_dir_all(d.join("cache/aae/git/x")).unwrap();
    for _ in 0..2 {
        let (ok, out, err) = aae(d, &["cache", "clear"]);
        assert!(ok, "{err}");
        assert_eq!(out.trim(), format!("cleared {}", d.join("cache/aae/git").display()));
    }
    assert!(!d.join("cache/aae/git").exists());
    let (ok, out, _) = aae(d, &["--json", "cache", "clear"]);
    assert!(ok && out.contains("\"cleared\""), "{out}");
}

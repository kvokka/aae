//! `aae fetch` on local git repositories: pins in `aae.lock`, checkouts, `--update` (spec §8.2, §11.6).

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// Runs git or `aae` away from the user's git config, whose hooks would run on the commits here.
fn isolated(mut command: Command, dir: &Path) -> Output {
    command
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
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

/// `aae fetch` with `args`: whether it succeeded and its stdout.
fn fetch(dir: &Path, args: &[&str]) -> (bool, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
    command.arg("fetch").args(args);
    let out = isolated(command, dir);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() || !stderr.is_empty(), "a failure says why");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn fetch_pins_checks_out_and_updates_only_when_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let pack = repo(
        d,
        "pack",
        &[
            ("pack.toml", "name = \"remote\"\nversion = \"0.1.0\"\n"),
            (
                "formulas/check.toml",
                "name = \"check\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n",
            ),
        ],
    );
    let plugin = repo(
        d,
        "plugin",
        &[("plugin.toml", "name = \"mailer\"\nversion = \"0.1.0\"\n")],
    );
    let (config_dir, shop) = (d.join("config/aae"), d.join("config/aae/shop"));
    fs::create_dir_all(&shop).unwrap();
    let source =
        |name: &str, rev: &str| format!("git = \"{}\", rev = \"{rev}\"", d.join("remote").join(name).display());
    let config = format!(
        "projects = [\"shop\"]\n[[plugin]]\n{}\n",
        source("plugin", "main").replace(", ", "\n")
    );
    fs::write(config_dir.join("config.toml"), config).unwrap();
    fs::write(
        shop.join("aae.toml"),
        format!("packs = [{{ {} }}]\n", source("pack", "main")),
    )
    .unwrap();
    let lock = |dir: &Path| fs::read_to_string(dir.join("aae.lock")).unwrap();

    assert!(
        aae_core::config::load(&config_dir.join("config.toml")).is_err(),
        "nothing is fetched yet"
    );
    let (ok, out) = fetch(d, &[]);
    assert!(ok, "{out}");
    assert!(out.contains(&format!("main -> {pack} (fetched)")), "{out}");
    assert!(out.contains(&format!("main -> {plugin} (fetched)")), "{out}");
    assert!(lock(&shop).contains(&format!("commit = \"{pack}\"")));
    assert!(lock(&config_dir).contains(&format!("commit = \"{plugin}\"")));
    let loaded = aae_core::config::load(&config_dir.join("config.toml")).unwrap();
    assert_eq!(loaded.plugins[0].name, "mailer");
    assert!(loaded.plugins[0].root.ends_with(format!(".aae/git/{plugin}")));
    assert!(loaded.projects[0].packs[1].formulas.contains_key("check"));

    let newer = commit(
        d,
        &d.join("remote/pack"),
        &[(
            "formulas/later.toml",
            "name = \"later\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n",
        )],
    );
    let (ok, out) = fetch(d, &[]);
    assert!(
        ok && out.contains(&format!("main -> {pack} (kept)")),
        "a pin moves only with --update: {out}"
    );
    let (ok, out) = fetch(d, &["--update"]);
    assert!(ok, "{out}");
    assert!(out.contains(&format!("main -> {newer} (updated)")), "{out}");
    assert!(out.contains(&format!("main -> {plugin} (kept)")), "{out}");
    assert!(lock(&shop).contains(&format!("commit = \"{newer}\"")));
    let loaded = aae_core::config::load(&config_dir.join("config.toml")).unwrap();
    assert!(loaded.projects[0].packs[1].formulas.contains_key("later"));

    // A source no longer declared loses its pin; a revision that names no commit fails the command.
    fs::write(shop.join("aae.toml"), "").unwrap();
    assert!(fetch(d, &[]).0);
    assert!(!lock(&shop).contains("[[source]]"));
    fs::write(
        shop.join("aae.toml"),
        format!("packs = [{{ {} }}]\n", source("pack", "nope")),
    )
    .unwrap();
    let (ok, _) = fetch(d, &[]);
    assert!(!ok, "no commit `nope`");
}

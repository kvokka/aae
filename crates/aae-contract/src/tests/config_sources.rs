//! Packs and plugins from git: the core loads them from the directories the caller downloaded them to
//! (docs/design/config.md, docs/guide/plugins.md).

use super::config::{BASE, fails};
use crate::config::{Checkouts, Config, ConfigError, GitSource, git_sources, load_for};
use std::{fs, path::Path};

const KIT: &str = "https://example.com/kit.git";
const MAIL: &str = "https://example.com/mail.git";

/// BASE with the pack and the plugin declared from git, their files moved to `dl/kit` and `dl/mail`.
fn write(dir: &Path, project: &str) {
    for (path, text) in BASE {
        let path = (path.strip_prefix("kit/").map(|p| format!("dl/kit/{p}")))
            .or_else(|| path.strip_prefix("plugins/mail/").map(|p| format!("dl/mail/{p}")))
            .unwrap_or_else(|| path.to_string());
        let text = match path.as_str() {
            "config.toml" => text.replace("path = \"plugins/mail\"", &format!("git = \"{MAIL}\"\nrev = \"v1\"")),
            "shop/aae.toml" => text.replace(r#"packs = ["../kit"]"#, project),
            _ => text.to_string(),
        };
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

fn source(git: &str, rev: Option<&str>) -> GitSource {
    GitSource {
        git: git.to_owned(),
        rev: rev.map(str::to_owned),
    }
}

/// The config in `dir` with the pack `{ git = KIT }` and the plugin `MAIL` at `v1` where `found` says.
fn load(dir: &Path, found: [Option<Result<&str, &str>>; 2]) -> Result<Config, Vec<ConfigError>> {
    write(dir, &format!(r#"packs = [{{ git = "{KIT}" }}]"#));
    let mut checkouts = Checkouts::new();
    let sources = [source(KIT, None), source(MAIL, Some("v1"))];
    for (s, f) in sources.into_iter().zip(found) {
        if let Some(f) = f {
            checkouts.insert(s, f.map(|d| dir.join(d)).map_err(str::to_owned));
        }
    }
    load_for(&dir.join("config.toml"), &[], checkouts)
}

#[test]
fn a_git_source_loads_from_the_directory_it_was_downloaded_to() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = load(d, [Some(Ok("dl/kit")), Some(Ok("dl/mail"))]).unwrap();
    assert_eq!(config.plugins[0].root, d.join("dl/mail"));
    let pack = &config.projects[0].packs[1];
    assert_eq!((pack.name.as_str(), &pack.root), ("kit", &d.join("dl/kit")));
    assert!(pack.formulas.contains_key("implement"));

    let errors = load(d, [None, None]).expect_err("nothing is downloaded");
    let missing = errors.iter().filter(|e| e.message.ends_with("is not downloaded"));
    assert_eq!(missing.count(), 2, "the plugin and the pack: {errors:?}");

    let errors = load(d, [Some(Err("boom")), Some(Ok("dl/mail"))]).expect_err("the pack failed to download");
    let expected = format!("`{KIT}`: boom");
    assert!(errors.iter().any(|e| e.message == expected), "{errors:?}");
}

#[test]
fn a_plugin_from_git_must_be_pinned() {
    fails(
        &[("config.toml", "[[plugin]]\ngit = \"x\"\n")],
        "a plugin from git needs `rev`",
    );
    let dir = tempfile::tempdir().unwrap();
    let errors = load(dir.path(), [None, None]).expect_err("nothing is downloaded");
    assert!(errors.iter().all(|e| !e.message.contains("needs `rev`")), "{errors:?}");
}

#[test]
fn git_sources_names_every_declared_source() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, &format!(r#"packs = [{{ git = "{KIT}" }}]"#));
    let found = git_sources(&d.join("config.toml"));
    assert_eq!(found, [source(KIT, None), source(MAIL, Some("v1"))].into());

    fs::write(d.join("shop/aae.toml"), "packs = [").unwrap();
    let found = git_sources(&d.join("config.toml"));
    assert_eq!(
        found,
        [source(MAIL, Some("v1"))].into(),
        "a broken project hides only its own"
    );
}

#[test]
fn a_plugin_has_a_path_or_a_git_source() {
    let either = "a plugin has either `path`, or `git` with `rev`";
    fails(
        &[(
            "config.toml",
            "[[plugin]]\npath = \"plugins/mail\"\ngit = \"x\"\nrev = \"v1\"\n",
        )],
        either,
    );
    fails(
        &[("config.toml", "[[plugin]]\npath = \"plugins/mail\"\nrev = \"v1\"\n")],
        either,
    );
}

#[test]
fn a_pack_with_an_unknown_key_says_what_a_pack_is() {
    let project = BASE[3].1.replace(
        r#"packs = ["../kit"]"#,
        r#"packs = [{ git = "x", rev = "main", extra = 1 }]"#,
    );
    fails(&[("shop/aae.toml", &project)], "a pack is a path or `{ git, rev }`");
}

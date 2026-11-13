//! Packs and plugins from git, pinned in `aae.lock` and checked out by `aae fetch` (spec §8.2, §11.6).

use super::config::{BASE, fails, load_with};

const PACK: &str = "1111111111111111111111111111111111111111";
const PLUGIN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONFIG: &str =
    "projects = [\"shop\"]\n[harness.claude]\n[[plugin]]\ngit = \"https://example.com/mail.git\"\nrev = \"v1\"\n";

/// BASE with the pack and the plugin declared from git; `fetched` adds their locks and checkouts.
fn files(fetched: bool) -> Vec<(String, String)> {
    let project = BASE[3].1.replace(
        r#"packs = ["../gascity"]"#,
        r#"packs = [{ git = "https://example.com/gascity.git", rev = "main" }]"#,
    );
    let mut files = vec![
        ("config.toml".to_owned(), CONFIG.to_owned()),
        ("shop/aae.toml".to_owned(), project),
    ];
    if fetched {
        let lock = |git: &str, rev: &str, commit: &str| {
            format!("[[source]]\ngit = \"https://example.com/{git}.git\"\nrev = \"{rev}\"\ncommit = \"{commit}\"\n")
        };
        files.push(("aae.lock".to_owned(), lock("mail", "v1", PLUGIN)));
        files.push(("shop/aae.lock".to_owned(), lock("gascity", "main", PACK)));
        for (path, text) in BASE {
            let moved = (path
                .strip_prefix("gascity/")
                .map(|p| format!("shop/.aae/git/{PACK}/{p}")))
            .or_else(|| {
                path.strip_prefix("plugins/mail/")
                    .map(|p| format!(".aae/git/{PLUGIN}/{p}"))
            });
            files.extend(moved.map(|p| (p, text.to_string())));
        }
    }
    files
}

fn load(files: &[(String, String)]) -> Result<crate::config::Config, Vec<crate::config::ConfigError>> {
    let changes: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    load_with(&changes)
}

#[test]
fn a_git_source_loads_from_its_pinned_checkout() {
    let errors = load(&files(false)).expect_err("nothing is fetched");
    let unfetched = errors
        .iter()
        .filter(|e| e.message.contains("is not fetched: run `aae fetch`"));
    assert_eq!(unfetched.count(), 2, "the plugin and the pack: {errors:?}");

    let config = load(&files(true)).unwrap();
    assert!(config.plugins[0].root.ends_with(format!(".aae/git/{PLUGIN}")));
    let pack = &config.projects[0].packs[1];
    assert_eq!(pack.name, "gascity");
    assert!(pack.root.ends_with(format!("shop/.aae/git/{PACK}")));
    assert!(pack.formulas.contains_key("implement"));
}

#[test]
fn a_pin_without_its_checkout_or_a_bad_lock_is_an_error() {
    let mut no_checkout = files(true);
    no_checkout.retain(|(p, _)| !p.starts_with(".aae/"));
    let errors = load(&no_checkout).expect_err("the plugin is not checked out");
    assert!(errors.iter().any(|e| {
        e.message
            .contains("`https://example.com/mail.git` at `v1` is not fetched")
    }));

    let mut bad = files(true);
    bad[2].1 = bad[2].1.replace(PLUGIN, "../../etc");
    let errors = load(&bad).expect_err("a commit names a directory");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("`../../etc` is not a full commit id")),
        "{errors:?}"
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
    fails(&[("config.toml", "[[plugin]]\ngit = \"x\"\n")], either);
    fails(
        &[("config.toml", "[[plugin]]\npath = \"plugins/mail\"\nrev = \"v1\"\n")],
        either,
    );
}

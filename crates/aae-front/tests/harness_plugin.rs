//! The skills plugin of Claude Code and Codex in `harness-plugin/` (docs/guide/recovery.md): its manifests are the
//! shape both harnesses install, and its skills name only commands the CLI has.

use aae_front::cli::{Cli, has_command};
use clap::Parser;
use serde_json::Value;
use std::{fs, path::Path};

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// The repository is a marketplace of one plugin: both `claude plugin marketplace add` and `codex plugin marketplace
/// add` read `.claude-plugin/marketplace.json` (Claude Code 2.1, Codex 0.168).
#[test]
fn the_manifests_are_what_claude_and_codex_install() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let market = json(&root.join(".claude-plugin/marketplace.json"));
    assert!(
        market["name"].is_string() && market["owner"]["name"].is_string(),
        "{market}"
    );
    let [entry] = market["plugins"].as_array().unwrap().as_slice() else {
        panic!("one plugin: {market}");
    };
    let source = entry["source"].as_str().unwrap();
    assert!(source.starts_with("./") && !source.contains(".."), "{source}");
    let dir = root.join(source);
    let plugin = json(&dir.join(".claude-plugin/plugin.json"));
    let name = plugin["name"].as_str().unwrap();
    assert!(
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "{name}"
    );
    assert_eq!(entry["name"], name, "the marketplace entry names the plugin");
    // Both harnesses cache a plugin by its version: a release that changes a skill changes the version.
    assert_eq!(plugin["version"], env!("CARGO_PKG_VERSION"));
    for skill in fs::read_dir(dir.join("skills")).unwrap() {
        let skill = skill.unwrap().path();
        let text = fs::read_to_string(skill.join("SKILL.md")).unwrap();
        let front = text
            .strip_prefix("---\n")
            .and_then(|t| t.split_once("\n---\n"))
            .unwrap()
            .0;
        let field = |key: &str| front.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(": "));
        assert_eq!(
            field("name"),
            skill.file_name().unwrap().to_str(),
            "{}",
            skill.display()
        );
        assert!(
            field("description").is_some_and(|d| !d.is_empty()),
            "{}",
            skill.display()
        );
    }
}

/// Words of a shell line: split on spaces, a single-quoted part is one word.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted) = (Vec::new(), String::new(), false);
    for c in line.chars() {
        match c {
            '\'' => quoted = !quoted,
            ' ' if !quoted => out.extend((!word.is_empty()).then(|| std::mem::take(&mut word))),
            c => word.push(c),
        }
    }
    out.extend((!word.is_empty()).then_some(word));
    out
}

#[test]
fn every_command_a_skill_names_exists() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness-plugin/skills");
    let mut seen = 0;
    for skill in fs::read_dir(&skills).unwrap() {
        let text = fs::read_to_string(skill.unwrap().path().join("SKILL.md")).unwrap();
        let mut block = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with("```") {
                block = line == "```sh";
                continue;
            }
            // A command line of a `sh` block parses with its options and values.
            if block && line.starts_with("aae ") {
                if let Err(e) = Cli::try_parse_from(words(line)) {
                    panic!("`{line}`: {e}");
                }
                seen += 1;
            }
            // A command in running text names an existing command.
            for span in line.split('`').skip(1).step_by(2) {
                if let Some(rest) = span.strip_prefix("aae ") {
                    let command: Vec<_> = rest.split(' ').take_while(|w| !w.starts_with(['-', '<'])).collect();
                    assert!(has_command(&command.join(" ")), "no command `{span}`");
                    seen += 1;
                }
            }
        }
    }
    assert!(seen > 0, "no command found in {}", skills.display());
}

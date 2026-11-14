//! The tested range of a harness binary (contracts 4.5): the daemon warns at start when the configured binary is
//! outside the `versions` of its driver's capabilities.

use std::path::Path;

/// Why `binary` is not a tested version, if it is not.
pub fn untested(binary: &Path, tested: [(u32, u32, u32); 2]) -> Option<String> {
    let shown = binary.display();
    let out = match std::process::Command::new(binary).arg("--version").output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        Err(e) => return Some(format!("{shown}: {e}")),
    };
    let dotted = |(a, b, c): (u32, u32, u32)| format!("{a}.{b}.{c}");
    match parse(&out) {
        Some(v) if (tested[0]..=tested[1]).contains(&v) => None,
        _ => Some(format!(
            "{shown} is `{out}`; tested: {} to {}",
            dotted(tested[0]),
            dotted(tested[1])
        )),
    }
}

/// The first `major.minor.patch` word of `--version` output: `2.1.322 (Some Tool)`, `some-cli 0.167.2`.
fn parse(text: &str) -> Option<(u32, u32, u32)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word.split('.').map(|p| p.parse().ok());
        let version = (parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    })
}

#[test]
fn a_version_is_read_from_the_version_output() {
    assert_eq!(parse("2.1.322 (Claude Code)"), Some((2, 1, 322)));
    assert_eq!(parse("codex-cli 0.167.2"), Some((0, 167, 2)));
    assert_eq!(parse("claude"), None);
    assert_eq!(parse("2.1"), None);
}

use crate::version::{parse, supported};

#[test]
fn a_version_is_read_from_the_version_output() {
    assert_eq!(parse("2.1.322 (Claude Code)"), Some((2, 1, 322)));
    assert_eq!(parse("codex-cli 0.167.2"), Some((0, 167, 2)));
    assert_eq!(parse("claude"), None);
    assert_eq!(parse("2.1"), None);
}

#[test]
fn a_harness_version_is_supported_from_the_minimum_up_with_no_upper_bound() {
    let minimum = (2, 1, 329);
    for (out, want) in [
        ("2.1.328 (Claude Code)", false),
        ("2.0.999 (Claude Code)", false),
        ("1.9.400 (Claude Code)", false),
        ("2.1.329 (Claude Code)", true),
        ("2.1.330 (Claude Code)", true),
        ("2.2.0 (Claude Code)", true),
        ("3.0.0 (Claude Code)", true),
        ("Claude Code", false),
    ] {
        assert_eq!(supported(out, minimum), want, "{out}");
    }
}

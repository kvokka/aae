//! Triggers checked against the events and the formulas they name (spec §9.5).

use super::config::{BASE, fails, load_with};

/// The project file with its trigger replaced by `trigger`.
fn project_trigger(trigger: &str) -> String {
    let start = BASE[3].1.find("[[trigger]]").unwrap();
    format!("{}[[trigger]]\n{trigger}\n", &BASE[3].1[..start])
}

#[test]
fn triggers_are_checked_against_events_and_formulas() {
    let cases = [
        (
            "name = \"t\"\non = \"bead.ready\"\nwhere = { \"bead.nope\" = 1 }\nrun = \"audit\"",
            "has no `bead.nope`",
        ),
        (
            "name = \"t\"\non = \"run.passed\"\nwhere = { run = \"r1\" }\nrun = \"audit\"",
            "has no `run`",
        ),
        (
            "name = \"t\"\non = \"bead.gone\"\nrun = \"audit\"",
            "unknown event type `bead.gone`",
        ),
        (
            "name = \"t\"\nevery = \"1h\"\nrun = \"implement\"\ninputs = { bead = \"{{ event.bead.id }}\" }",
            "no event",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\nrun = \"implement\"\ninputs = { bead = \"{{ event.bead.nope }}\" }",
            "no `bead.nope`",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\nrun = \"implement\"\ninputs = { bead = \"x\", extra = 1 }",
            "no input `extra`",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\nrun = \"implement\"",
            "required input `bead`",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\ncron = \"0 3 * * *\"\nrun = \"audit\"",
            "exactly one of",
        ),
        ("name = \"t\"\ncron = \"0 3 * *\"\nrun = \"audit\"", "five cron fields"),
        ("name = \"t\"\nevery = \"3d\"\nrun = \"audit\"", "unit is one of"),
        (
            "name = \"t\"\ncron = \"0 3 * * *\"\nwhere = { project = \"x\" }\nrun = \"audit\"",
            "`where` needs `on`",
        ),
        (
            "name = \"t\"\ncron = \"0 3 * * *\"\nrun = \"audit\"\nrig = \"frontend\"",
            "no rig `frontend`",
        ),
        (
            "name = \"t\"\ncron = \"0 3 * * *\"\nrun = \"nothing\"",
            "no formula `nothing`",
        ),
    ];
    for (trigger, needle) in cases {
        fails(&[("shop/aae.toml", project_trigger(trigger).as_str())], needle);
    }
    let with_rig = BASE[5]
        .1
        .replace("run = \"audit\"", "run = \"audit\"\nrig = \"backend\"");
    fails(&[("kit/pack.toml", with_rig.as_str())], "gets its rig from the project");
}

#[test]
fn cron_reads_the_system_zone_unless_timezone_names_one() {
    let with = |zone: &str| {
        BASE[0]
            .1
            .replace("projects", &format!("timezone = \"{zone}\"\nprojects"))
    };
    let config = load_with(&[("config.toml", with("Europe/Berlin").as_str())]).unwrap();
    assert_eq!(config.timezone.iana_name(), Some("Europe/Berlin"));
    fails(&[("config.toml", with("Mars/Olympus").as_str())], "`timezone`");
    assert_eq!(load_with(BASE).unwrap().timezone, jiff::tz::TimeZone::system());
}

/// Where each trigger runs (spec §9.3): a project trigger without a `rig` in the root, under the root's name, and
/// with `rig` naming the root there too; a pack trigger only on the rigs that list it.
#[test]
fn a_project_trigger_without_a_rig_runs_in_the_root() {
    let places = |changes: &[(&str, &str)]| {
        let config = load_with(changes).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        crate::open(&db).unwrap();
        let listed = crate::run::Reader::open(&db).unwrap().triggers(&config).unwrap();
        let mut at: Vec<String> = (listed.iter())
            .map(|t| format!("{} {}", t["trigger"], t["rig"]))
            .collect();
        at.sort();
        at
    };
    let expect = |root: &str| {
        vec![
            r#""kit.nightly" "backend""#.to_owned(),
            format!(r#""shop.implement-next" "{root}""#),
        ]
    };
    assert_eq!(places(&[]), expect("root"));
    let named = BASE[3]
        .1
        .replacen("[rigs.backend]", "[root]\nname = \"main\"\n[rigs.backend]", 1);
    assert_eq!(places(&[("shop/aae.toml", &named)]), expect("main"));
    let to_root = named.replace("run = \"implement\"\n", "run = \"implement\"\nrig = \"main\"\n");
    assert_eq!(places(&[("shop/aae.toml", &to_root)]), expect("main"));
}

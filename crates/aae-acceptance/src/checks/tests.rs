use super::*;

/// The rows of docs/acceptance.md, criterion 21 withdrawn: the report shows each, run or not.
const ROWS: &[&str] = &[
    "1", "2", "3", "4", "4a", "5a", "5b", "6", "7", "8", "9", "10", "11", "12", "13", "14", "16", "17", "18", "19",
    "20", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32", "33", "34", "35",
];

#[test]
fn every_row_of_the_acceptance_table_is_in_the_report() {
    let checked = ALL.iter().chain(crate::scenarios::ALL).map(|s| s.criterion);
    let known: Vec<&str> = checked.collect();
    for row in ROWS {
        assert!(known.contains(row), "criterion {row} has no check or scenario");
    }
    assert!(known.iter().all(|c| ROWS.contains(c)), "{known:?}");
}

/// Each demo with a `run.sh` has a live run among the scenarios on request (criterion 20).
#[test]
fn every_demo_has_a_live_run() {
    for entry in std::fs::read_dir(root().join("demos")).unwrap() {
        let dir = entry.unwrap().path();
        if !dir.join("run.sh").is_file() {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let number = name.split('-').next().unwrap();
        let title = format!("demo {number}:");
        assert!(
            ALL.iter()
                .any(|s| matches!(s.launch, Launch::Request) && s.title.starts_with(&title)),
            "{name} has no live run"
        );
    }
}

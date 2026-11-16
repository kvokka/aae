use super::*;

/// The rows of docs/acceptance.md, criterion 21 withdrawn: the report shows each, run or not.
const ROWS: &[&str] = &[
    "1", "2", "3", "4", "4a", "5a", "5b", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18",
    "19", "20", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32", "33", "34", "35",
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

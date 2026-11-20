//! The TOON text of structured values in prompts (contracts 2.3): tables, lists, inline arrays and quoting.

use crate::toon::encode;
use serde_json::json;

#[test]
fn comments_are_a_table_and_a_bead_with_parents_a_list() {
    let comments = json!([
        { "author": "alice", "created_at": "2026-11-19T03:59:18Z", "text": "go left" },
        { "author": "loop-manager:run_1:2", "created_at": "2026-11-19T04:10:00Z", "text": "Tests: 2 red, see below\n- a::b" }
    ]);
    assert_eq!(
        encode(&comments),
        "[2]{author,created_at,text}:\n  alice,\"2026-11-19T03:59:18Z\",go left\n  \
         \"loop-manager:run_1:2\",\"2026-11-19T04:10:00Z\",\"Tests: 2 red, see below\\n- a::b\""
    );
    let bead = json!({
        "comments": [],
        "id": "t-1",
        "labels": ["auto", "ui"],
        "parents": [{ "comments": [{ "author": "bob", "text": "why" }], "id": "t-0", "labels": [], "title": "Epic: all of it" }],
        "priority": 2
    });
    assert_eq!(
        encode(&bead),
        "comments: []\nid: t-1\nlabels[2]: auto,ui\nparents[1]:\n  - comments[1]{author,text}:\n      bob,why\n    \
         id: t-0\n    labels: []\n    title: \"Epic: all of it\"\npriority: 2"
    );
    let output = json!({ "exit_code": 1, "stderr": "", "stdout": "FAILED 3 tests\n" });
    assert_eq!(
        encode(&output),
        "exit_code: 1\nstderr: \"\"\nstdout: \"FAILED 3 tests\\n\""
    );
}

#[test]
fn a_string_is_quoted_when_it_would_not_read_back_as_itself() {
    assert_eq!(encode(&json!("\u{feff}hello")), "\"\u{feff}hello\"");
    let strings = json!([
        "plain words",
        "",
        " pad",
        "true",
        "12",
        "-x",
        "# head",
        "a: b",
        "a,b",
        "[x]",
        "q\"q",
        "tab\t",
        "\u{1}"
    ]);
    assert_eq!(
        encode(&strings),
        "[13]: plain words,\"\",\" pad\",\"true\",\"12\",\"-x\",\"# head\",\"a: b\",\"a,b\",\"[x]\",\"q\\\"q\",\
         \"tab\\t\",\"\\u0001\""
    );
}

#[test]
fn mixed_arrays_are_lists_and_odd_keys_are_quoted() {
    let mixed = json!([1, { "a": 1 }, [2, 3], [], {}, { "b": { "c": null }, "d": -0.0, "e": 1.5 }]);
    assert_eq!(
        encode(&mixed),
        "[6]:\n  - 1\n  - a: 1\n  - [2]: 2,3\n  - [0]:\n  -\n  - b:\n      c: null\n    d: 0\n    e: 1.5"
    );
    let keys = json!({ "key with space": 1, "9": true, "a.b": { "c": [] } });
    assert_eq!(encode(&keys), "\"9\": true\na.b:\n  c: []\n\"key with space\": 1");
    assert_eq!(encode(&json!("a: b")), "\"a: b\"");
    assert_eq!(encode(&json!([])), "[]");
    assert_eq!(encode(&json!({})), "");
}

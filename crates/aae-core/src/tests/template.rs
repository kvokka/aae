use crate::template::{Filter, Inputs, SyntaxError, Template, Unresolved, event_view};
use serde_json::{Value, json};

const PROMPT: &[&str] = &["inputs", "bead", "rig", "run", "project"];

fn ready_event() -> Value {
    json!({
        "schema": 1, "seq": 7, "ts": "2027-01-15T08:00:00.000Z", "type": "bead.ready",
        "project": "web", "rig": null, "run": null, "correlation": null,
        "data": { "bead": { "id": "bd-1", "epic": "bd-0", "labels": ["auto", "ui"], "priority": 2 } }
    })
}

fn filter(table: Value) -> Result<Filter, SyntaxError> {
    Filter::parse(table.as_object().unwrap())
}

#[test]
fn the_event_view_is_flat() {
    let view = event_view(&ready_event());
    assert_eq!(view["seq"], json!(7));
    assert_eq!(view["type"], json!("bead.ready"));
    assert_eq!(view["bead"]["epic"], json!("bd-0"));
    assert!(view.get("data").is_none() && view.get("schema").is_none());
}

#[test]
fn where_matches_scalars_arrays_and_requires_every_entry() {
    let view = event_view(&ready_event());
    let yes = |t: Value| filter(t).unwrap().matches(&view);
    assert!(yes(json!({})), "an empty filter matches everything");
    assert!(
        yes(json!({ "bead.labels": "auto" })),
        "an array value contains the scalar"
    );
    assert!(!yes(json!({ "bead.labels": "backend" })));
    assert!(yes(json!({ "bead.labels": ["backend", "ui"] })), "contains any element");
    assert!(yes(json!({ "bead.priority": [1, 2], "project": "web" })));
    assert!(
        !yes(json!({ "bead.priority": 2, "project": "api" })),
        "AND of all entries"
    );
    assert!(!yes(json!({ "rig": "main" })), "null does not match");
    assert!(!yes(json!({ "bead.owner": "me" })), "absent does not match");
    assert!(yes(json!({ "bead.priority": 2 })));
}

#[test]
fn where_rejects_what_it_cannot_match() {
    assert!(filter(json!({ "bead.labels": { "any": "x" } })).is_err());
    assert!(filter(json!({ "bead.labels": [] })).is_err());
    assert!(filter(json!({ "bead.priority": 1.5 })).is_err());
    assert!(filter(json!({ "bead..labels": "x" })).is_err());
    assert_eq!(
        filter(json!({ "bead.epic": "x" })).unwrap().paths().collect::<Vec<_>>(),
        ["bead.epic"]
    );
}

#[test]
fn a_prompt_template_substitutes_scalars_and_fails_on_missing_values() {
    let context = json!({ "bead": { "id": "bd-1", "priority": 2 }, "rig": { "path": "/src" }, "inputs": {} });
    let t = Template::parse("Fix {{bead.id}} (p{{ bead.priority }}) in {{rig.path}}.", PROMPT).unwrap();
    assert_eq!(t.render(&context).unwrap(), "Fix bd-1 (p2) in /src.");
    assert_eq!(t.paths().collect::<Vec<_>>(), ["bead.id", "bead.priority", "rig.path"]);
    let missing = Template::parse("{{ inputs.goal }}", PROMPT).unwrap();
    assert_eq!(
        missing.render(&context),
        Err(Unresolved {
            path: "inputs.goal".into(),
            not_scalar: false
        })
    );
    let object = Template::parse("{{ rig }}", PROMPT).unwrap();
    assert_eq!(
        object.render(&context),
        Err(Unresolved {
            path: "rig".into(),
            not_scalar: true
        }),
        "an object is not text"
    );
}

#[test]
fn template_syntax_is_checked_at_load() {
    let escaped = Template::parse(r#"literal {{"{{"}} and }} stay"#, PROMPT).unwrap();
    assert_eq!(escaped.render(&json!({})).unwrap(), "literal {{ and }} stay");
    assert!(Template::parse("{{ bead.id", PROMPT).is_err(), "unclosed");
    assert!(Template::parse("{{ }}", PROMPT).is_err(), "empty");
    assert!(Template::parse("{{ bead.id | upper }}", PROMPT).is_err(), "no filters");
    assert!(Template::parse("{{ event.bead }}", PROMPT).is_err(), "unknown root");
}

#[test]
fn inputs_keep_json_types_map_recursively_and_report_unresolved_paths() {
    let spec = json!({
        "bead": { "next_ready": true, "epic": "{{event.bead.epic}}" },
        "labels": "{{ event.bead.labels }}",
        "note": "from {{event.type}} #{{event.seq}}",
        "list": ["{{event.bead.priority}}", 3]
    });
    let inputs = Inputs::parse(&spec, true).unwrap();
    let view = event_view(&ready_event());
    assert_eq!(
        inputs.resolve(Some(&view)).unwrap(),
        json!({
            "bead": { "next_ready": true, "epic": "bd-0" },
            "labels": ["auto", "ui"],
            "note": "from bead.ready #7",
            "list": [2, 3]
        })
    );
    let other = event_view(&json!({ "seq": 8, "type": "bead.ready", "data": { "bead": { "id": "bd-2" } } }));
    assert_eq!(
        inputs.resolve(Some(&other)),
        Err(Unresolved {
            path: "event.bead.epic".into(),
            not_scalar: false
        })
    );
}

#[test]
fn schedule_inputs_have_no_placeholders() {
    assert!(Inputs::parse(&json!({ "epic": "{{ event.bead.epic }}" }), false).is_err());
    let fixed = Inputs::parse(&json!({ "scope": "all", "depth": 1 }), false).unwrap();
    assert_eq!(fixed.resolve(None).unwrap(), json!({ "scope": "all", "depth": 1 }));
    assert!(
        Inputs::parse(&json!({ "x": "{{ bead.id }}" }), true).is_err(),
        "inputs start at `event`"
    );
}

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

#[test]
fn a_loaded_where_on_the_run_view_matches_a_journal_envelope() {
    use crate::config::Source;
    let trigger = "name = \"t\"\non = \"run.passed\"\nwhere = { \"run.formula\" = \"implement\" }\nrun = \"audit\"";
    let project = super::config::BASE[3].1;
    let project = format!(
        "{}[[trigger]]\n{trigger}\n",
        &project[..project.find("[[trigger]]").unwrap()]
    );
    let config = super::config::load_with(&[("shop/aae.toml", project.as_str())]).unwrap();
    let Source::Event { filter, .. } = &config.projects[0].packs[0].triggers[0].source else {
        panic!("an event trigger");
    };
    let passed = |formula: &str| {
        let body = json!({
            "type": "run.passed", "project": "shop", "rig": "backend", "run": "r1", "correlation": null,
            "data": { "run": { "id": "r1", "formula": formula }, "outcome": { "result": "pass" } }
        });
        crate::journal::envelope(3, 0, &body.to_string())
    };
    assert!(filter.matches(&event_view(&passed("implement"))));
    assert!(!filter.matches(&event_view(&passed("audit"))));
    assert_eq!(event_view(&passed("implement"))["run"]["id"], json!("r1"));
}

#[test]
fn event_paths_follow_the_payload_schemas() {
    use crate::config::schema::event_path;
    assert_eq!(event_path("bead.ready", "bead.labels", false), Some(true));
    assert_eq!(event_path("bead.ready", "bead.labels.x", false), Some(false));
    assert_eq!(event_path("bead.ready", "seq", false), Some(true));
    assert_eq!(event_path("bead.ready", "seq.x", false), Some(false));
    assert_eq!(event_path("run.passed", "run", false), Some(true));
    assert_eq!(event_path("run.passed", "run.formula", false), Some(true));
    assert_eq!(event_path("run.passed", "outcome.data.x.y", false), Some(true));
    assert_eq!(event_path("gate.opened", "rig", false), Some(true));
    assert_eq!(event_path("gate.opened", "gate", false), Some(false));
    assert_eq!(event_path("no.such", "seq", false), None);
    assert_eq!(
        event_path("run.passed", "run", true),
        Some(false),
        "an object never matches"
    );
    assert_eq!(event_path("run.passed", "run.formula", true), Some(true));
    assert_eq!(
        event_path("run.state_changed", "run", true),
        Some(true),
        "the envelope run id"
    );
    assert_eq!(event_path("run.state_changed", "run.formula", false), Some(false));
}

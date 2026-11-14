//! The reply schemas of the OpenAPI document are written by hand: the replies of a real daemon must fit them.

use serde_json::Value;

/// `reply` to `method path` is not an empty list and fits the 200 reply schema of the generated document.
pub(crate) fn check(method: &str, path: &str, reply: &Value) {
    let doc = serde_json::to_value(aae_front::api::openapi()).unwrap();
    let at = format!("{} {path}", method.to_uppercase());
    let empty = reply.as_array().is_some_and(Vec::is_empty);
    assert!(!empty, "{at}: an empty list checks nothing");
    fits(&doc, &reply_schema(&doc, method, path), reply, &at);
}

/// The schema of the 200 reply of `method path` in the generated document.
pub(crate) fn reply_schema(doc: &Value, method: &str, path: &str) -> Value {
    let schema = &doc["paths"][path][method]["responses"]["200"]["content"]["application/json"]["schema"];
    assert!(!schema.is_null(), "{method} {path}: no 200 reply schema");
    schema.clone()
}

/// The JSON Schema type of `value`.
fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `reply` has a type the schema allows, every key of it is a declared property and every required property is
/// there, down nested objects.
pub(crate) fn fits(doc: &Value, schema: &Value, reply: &Value, at: &str) {
    let schema = match schema["$ref"].as_str() {
        Some(r) => &doc["components"]["schemas"][r.trim_start_matches("#/components/schemas/")],
        None => schema,
    };
    // An optional reference: `null`, or a value that fits one of the other branches.
    if let Some(branches) = schema["oneOf"].as_array() {
        let null = branches.iter().any(|b| b["type"] == "null");
        if !(reply.is_null() && null) {
            let other: Vec<_> = branches.iter().filter(|b| b["type"] != "null").collect();
            assert_eq!(other.len(), 1, "{at}: a oneOf this check does not read");
            fits(doc, other[0], reply, at);
        }
        return;
    }
    let types: Vec<&str> = match &schema["type"] {
        Value::String(t) => vec![t],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    let found = type_of(reply);
    let allowed = types.contains(&found) || (found == "integer" && types.contains(&"number"));
    assert!(
        types.is_empty() || allowed,
        "{at}: a {found}, the schema says {types:?}"
    );
    if let (Some(items), Some(reply)) = (schema.get("items"), reply.as_array()) {
        reply.iter().for_each(|r| fits(doc, items, r, at));
        return;
    }
    let (Some(properties), Some(reply)) = (schema["properties"].as_object(), reply.as_object()) else {
        return;
    };
    for (key, value) in reply {
        let property = properties.get(key);
        assert!(property.is_some(), "{at}: `{key}` is not in the schema");
        fits(doc, property.unwrap(), value, &format!("{at}.{key}"));
    }
    for required in schema["required"].as_array().into_iter().flatten() {
        assert!(
            reply.contains_key(required.as_str().unwrap()),
            "{at}: `{required}` is missing"
        );
    }
}

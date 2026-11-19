//! TOON (Token-Oriented Object Notation, https://toonformat.dev) text of a JSON value: an agent reads structured data
//! in it for fewer tokens than in JSON (docs/design/contracts.md 2.3). An encoder only, of the plain forms of the
//! format (spec 4.4): fields, inline arrays, tables and lists, with the comma and two spaces a level. The keys of an
//! object come in the order of the value, which is sorted.

use serde_json::{Map, Value};

/// `value` as TOON, without a final newline.
pub(crate) fn encode(value: &Value) -> String {
    let mut lines = Vec::new();
    match value {
        Value::Object(fields) => object(&mut lines, fields, 0),
        Value::Array(items) => array(&mut lines, String::new(), items, 0),
        scalar => lines.push(primitive(scalar)),
    }
    lines.join("\n")
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn is_scalar(value: &Value) -> bool {
    !value.is_object() && !value.is_array()
}

fn row<'a>(values: impl Iterator<Item = &'a Value>) -> String {
    values.map(primitive).collect::<Vec<_>>().join(",")
}

/// The fields of an object, one per line at `depth`.
fn object(lines: &mut Vec<String>, fields: &Map<String, Value>, depth: usize) {
    for (name, value) in fields {
        field(lines, format!("{}{}", indent(depth), key(name)), value, depth);
    }
}

/// One field whose line starts with `head` (the indent or the `- ` of a list item, then the key); what it holds
/// goes one level below `depth`.
fn field(lines: &mut Vec<String>, head: String, value: &Value, depth: usize) {
    match value {
        Value::Object(fields) => {
            lines.push(format!("{head}:"));
            object(lines, fields, depth + 1);
        }
        Value::Array(items) => array(lines, head, items, depth),
        scalar => lines.push(format!("{head}: {}", primitive(scalar))),
    }
}

/// An array of a field or of the root: scalars on the line of its header, objects of the same scalar fields as a
/// table, anything else as a list.
fn array(lines: &mut Vec<String>, head: String, items: &[Value], depth: usize) {
    let columns: Vec<&String> = items
        .first()
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::keys)
        .collect();
    let same = |o: &Map<String, Value>| o.keys().eq(columns.iter().copied()) && o.values().all(is_scalar);
    if columns.is_empty() || !items.iter().all(|item| item.as_object().is_some_and(same)) {
        return list(lines, head, items, depth);
    }
    let names: Vec<String> = columns.iter().map(|name| key(name)).collect();
    lines.push(format!("{head}[{}]{{{}}}:", items.len(), names.join(",")));
    let rows = items.iter().filter_map(Value::as_object);
    lines.extend(rows.map(|o| format!("{}{}", indent(depth + 1), row(o.values()))));
}

/// An array without a table: empty, scalars on the line of its header, or a list of `- ` items one level below
/// `depth`.
fn list(lines: &mut Vec<String>, head: String, items: &[Value], depth: usize) {
    if items.is_empty() {
        return lines.push(match head.as_str() {
            "" => "[]".to_owned(),
            item if item.ends_with("- ") => format!("{item}[0]:"),
            field => format!("{field}: []"),
        });
    }
    if items.iter().all(is_scalar) {
        return lines.push(format!("{head}[{}]: {}", items.len(), row(items.iter())));
    }
    lines.push(format!("{head}[{}]:", items.len()));
    let dash = format!("{}- ", indent(depth + 1));
    for item in items {
        match item {
            Value::Object(fields) if fields.is_empty() => lines.push(dash.trim_end().to_owned()),
            // The first field stands on the line of the hyphen, the others below it, one level in.
            Value::Object(fields) => {
                for (at, (name, value)) in fields.iter().enumerate() {
                    let start = if at == 0 { dash.clone() } else { indent(depth + 2) };
                    field(lines, format!("{start}{}", key(name)), value, depth + 2);
                }
            }
            Value::Array(inner) => list(lines, dash.clone(), inner, depth + 1),
            scalar => lines.push(format!("{dash}{}", primitive(scalar))),
        }
    }
}

/// A key as it is when it is a plain name, quoted otherwise.
fn key(name: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    match name.chars().next() {
        Some(first) if (first.is_ascii_alphabetic() || first == '_') && name.chars().all(plain) => name.to_owned(),
        _ => quoted(name),
    }
}

fn primitive(value: &Value) -> String {
    match value {
        Value::String(text) => {
            // Quoted: what would read as another type, as structure, as a comment or as two values, and blank edges.
            let plain = !text.is_empty()
                && text.trim() == text
                && !matches!(text.as_str(), "true" | "false" | "null")
                && text.parse::<f64>().is_err()
                && !text.starts_with(['-', '#'])
                && !text.contains([':', '"', '\\', '[', ']', '{', '}', ','])
                && !text.contains(|c: char| c < ' ');
            if plain { text.clone() } else { quoted(text) }
        }
        // No exponent and no `-0`: the format's form of a number.
        Value::Number(n) if n.is_f64() => match n.as_f64() {
            Some(f) if f != 0.0 => f.to_string(),
            _ => "0".to_owned(),
        },
        other => other.to_string(),
    }
}

fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

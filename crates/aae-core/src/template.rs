//! The filter and template language (docs/design/contracts.md 2): `where` matchers over the event view (1.2) and
//! `{{ path }}` substitution, shared by trigger `inputs` and prompt templates.

use serde_json::{Map, Value};
use std::fmt;

/// A placeholder or path that cannot be accepted when the config is loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError(pub String);

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SyntaxError {}

/// A placeholder that cannot be substituted: its path is absent or null in the values it is resolved against, or,
/// inside text, it holds an object or array (`not_scalar`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unresolved {
    pub path: String,
    pub not_scalar: bool,
}

impl Unresolved {
    fn absent(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            not_scalar: false,
        }
    }
}

impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.not_scalar {
            true => write!(f, "`{}` is an object or array, not text", self.path),
            false => write!(f, "`{}` has no value", self.path),
        }
    }
}

/// The event as filters and templates see it: envelope fields and the keys of `data` on one level, a `data` key hiding
/// the envelope field of the same name (the `RunView` of `run.*` payloads hides the run id, which is its `id`).
pub fn event_view(envelope: &Value) -> Value {
    let mut view = Map::new();
    if let Value::Object(fields) = envelope {
        for (key, value) in fields {
            if !matches!(key.as_str(), "data" | "schema") {
                view.insert(key.clone(), value.clone());
            }
        }
        if let Some(Value::Object(data)) = fields.get("data") {
            view.extend(data.clone());
        }
    }
    Value::Object(view)
}

/// The value at a dotted path; absent and null are the same.
fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(root, |value, key| value.get(key))
        .filter(|v| !v.is_null())
}

fn check_path(path: &str, roots: &[&str]) -> Result<(), SyntaxError> {
    let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !path.split('.').all(valid) {
        return Err(SyntaxError(format!("`{path}` is not a dotted path")));
    }
    let root = path.split('.').next().unwrap_or_default();
    if !roots.is_empty() && !roots.contains(&root) {
        return Err(SyntaxError(format!(
            "`{path}` starts with none of: {}",
            roots.join(", ")
        )));
    }
    Ok(())
}

/// `where`: every `path = matcher` entry matches (contracts 2.2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter(Vec<(String, Vec<Value>)>);

impl Filter {
    pub fn parse(table: &Map<String, Value>) -> Result<Self, SyntaxError> {
        let mut entries = Vec::new();
        for (path, matcher) in table {
            check_path(path, &[])?;
            let accepted = match matcher {
                Value::Array(items) => items.clone(),
                scalar => vec![scalar.clone()],
            };
            if accepted.is_empty() || !accepted.iter().all(|v| v.is_string() || v.is_i64() || v.is_boolean()) {
                return Err(SyntaxError(format!(
                    "`{path}`: a matcher is a string, integer or boolean, or a non-empty array of them"
                )));
            }
            entries.push((path.clone(), accepted));
        }
        Ok(Self(entries))
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(path, _)| path.as_str())
    }

    pub fn matches(&self, view: &Value) -> bool {
        self.0.iter().all(|(path, accepted)| match lookup(view, path) {
            Some(Value::Array(items)) => items.iter().any(|item| accepted.contains(item)),
            Some(value) => accepted.contains(value),
            None => false,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Text(String),
    Path(String),
}

/// A string with `{{ path }}` placeholders; `{{"{{"}}` is a literal `{{` (contracts 2.3).
#[derive(Clone, Debug, PartialEq)]
pub struct Template(Vec<Part>);

impl Template {
    /// Parses `text`; every placeholder must start with one of `roots`.
    pub fn parse(text: &str, roots: &[&str]) -> Result<Self, SyntaxError> {
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find("{{") {
            push_text(&mut parts, &rest[..open]);
            let after = &rest[open + 2..];
            let close = after
                .find("}}")
                .ok_or_else(|| SyntaxError(format!("unclosed `{{{{` in `{text}`")))?;
            match after[..close].trim() {
                "\"{{\"" => push_text(&mut parts, "{{"),
                path => {
                    check_path(path, roots)?;
                    parts.push(Part::Path(path.to_owned()));
                }
            }
            rest = &after[close + 2..];
        }
        push_text(&mut parts, rest);
        Ok(Self(parts))
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|p| match p {
            Part::Path(path) => Some(path.as_str()),
            Part::Text(_) => None,
        })
    }

    /// Renders as text; every placeholder must resolve to a scalar.
    pub fn render(&self, context: &Value) -> Result<String, Unresolved> {
        let mut out = String::new();
        for part in &self.0 {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Path(path) => match lookup(context, path) {
                    Some(Value::String(s)) => out.push_str(s),
                    Some(v @ (Value::Number(_) | Value::Bool(_))) => out.push_str(&v.to_string()),
                    Some(Value::Object(_) | Value::Array(_)) => {
                        return Err(Unresolved {
                            path: path.clone(),
                            not_scalar: true,
                        });
                    }
                    _ => return Err(Unresolved::absent(path)),
                },
            }
        }
        Ok(out)
    }

    /// A template that is exactly one placeholder keeps the JSON type of its value.
    fn resolve(&self, context: &Value) -> Result<Value, Unresolved> {
        match self.0.as_slice() {
            [Part::Path(path)] => lookup(context, path).cloned().ok_or_else(|| Unresolved::absent(path)),
            _ => self.render(context).map(Value::String),
        }
    }
}

fn push_text(parts: &mut Vec<Part>, text: &str) {
    if text.is_empty() {
        return;
    }
    match parts.last_mut() {
        Some(Part::Text(last)) => last.push_str(text),
        _ => parts.push(Part::Text(text.to_owned())),
    }
}

/// Trigger `inputs`: tables and arrays mapped recursively, strings as templates (contracts 2.3).
#[derive(Clone, Debug, PartialEq)]
pub enum Inputs {
    Template(Template),
    Table(Vec<(String, Inputs)>),
    List(Vec<Inputs>),
    Literal(Value),
}

impl Inputs {
    /// Placeholders start with `event`; a schedule trigger (`has_event == false`) may have none.
    pub fn parse(value: &Value, has_event: bool) -> Result<Self, SyntaxError> {
        Ok(match value {
            Value::String(text) => {
                let template = Template::parse(text, &["event"])?;
                if !has_event && template.paths().next().is_some() {
                    return Err(SyntaxError(format!(
                        "`{text}`: a schedule trigger has no event to substitute"
                    )));
                }
                Self::Template(template)
            }
            Value::Object(table) => Self::Table(
                table
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), Self::parse(v, has_event)?)))
                    .collect::<Result<_, SyntaxError>>()?,
            ),
            Value::Array(items) => Self::List(
                items
                    .iter()
                    .map(|v| Self::parse(v, has_event))
                    .collect::<Result<_, _>>()?,
            ),
            literal => Self::Literal(literal.clone()),
        })
    }

    /// Every placeholder path, `event.` included.
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Template(template) => template.paths().collect(),
            Self::Table(entries) => entries.iter().flat_map(|(_, v)| v.paths()).collect(),
            Self::List(items) => items.iter().flat_map(Self::paths).collect(),
            Self::Literal(_) => Vec::new(),
        }
    }

    /// The inputs of one firing; `view` is the event view of the firing event, `None` for a schedule.
    pub fn resolve(&self, view: Option<&Value>) -> Result<Value, Unresolved> {
        self.resolve_in(&serde_json::json!({ "event": view }))
    }

    fn resolve_in(&self, context: &Value) -> Result<Value, Unresolved> {
        Ok(match self {
            Self::Template(template) => template.resolve(context)?,
            Self::Table(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), v.resolve_in(context)?)))
                    .collect::<Result<_, Unresolved>>()?,
            ),
            Self::List(items) => Value::Array(items.iter().map(|v| v.resolve_in(context)).collect::<Result<_, _>>()?),
            Self::Literal(value) => value.clone(),
        })
    }
}

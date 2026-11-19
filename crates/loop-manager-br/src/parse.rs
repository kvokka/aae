//! What `br show --json` says of a bead (docs/design/tracker-br.md).

use loop_manager_contract::tracker::{Bead, Comment, Commented, TrackerError};
use serde_json::Value;

/// The bead field an `events` row of br changed.
pub(crate) fn field(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "status_changed" | "reopened" => "status",
        "assignee_changed" => "assignee",
        "priority_changed" => "priority",
        "label_added" | "label_removed" => "labels",
        "dependency_added" | "dependency_removed" => "dependencies",
        "commented" => "comments",
        "updated" => "fields",
        _ => return None,
    })
}

/// A bead of `br show --json`; its parent is its epic (br has no epic field, docs/design/tracker-br.md).
pub(crate) fn bead(v: &Value) -> Result<Bead, TrackerError> {
    let text = |k: &str| v[k].as_str().map(str::to_owned);
    let invalid = || TrackerError::Invalid {
        text: format!("not a bead: {v}"),
    };
    Ok(Bead {
        id: text("id").ok_or_else(invalid)?,
        title: text("title").unwrap_or_default(),
        description: text("description").unwrap_or_default(),
        status: text("status").ok_or_else(invalid)?,
        priority: v["priority"].as_i64().unwrap_or_default(),
        labels: v["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str().map(str::to_owned))
            .collect(),
        assignee: text("assignee").filter(|a| !a.is_empty()),
        epic: text("parent"),
        created_at: text("created_at").unwrap_or_default(),
        updated_at: text("updated_at").unwrap_or_default(),
        closed_at: text("closed_at"),
    })
}

/// A bead of `br show --json` with its comments, oldest first, as br lists them.
pub(crate) fn commented(v: &Value) -> Result<Commented, TrackerError> {
    let text = |c: &Value, k: &str| c[k].as_str().unwrap_or_default().to_owned();
    let comments = (v["comments"].as_array().into_iter().flatten())
        .map(|c| Comment {
            author: text(c, "author"),
            created_at: text(c, "created_at"),
            text: text(c, "text"),
        })
        .collect();
    Ok(Commented {
        bead: bead(v)?,
        comments,
    })
}

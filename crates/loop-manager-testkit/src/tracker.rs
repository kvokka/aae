//! A tracker in memory (contracts 5) for core tests: claims, conditional release and close, the ready set in
//! creation order, creation once per key, and an outage switch. As in br, a parent with open children is not ready
//! and is released, not closed.

use loop_manager_contract::{
    BoxFuture,
    tracker::{Bead, Change, Comment, Commented, Lineage, Tracker, TrackerError},
};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Default)]
pub struct MemoryTracker {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    beads: Vec<Bead>,
    log: Vec<Change>,
    /// The beads of `create`, by key.
    keys: Vec<(String, String)>,
    /// The comments, oldest first, by bead id.
    comments: Vec<(String, Comment)>,
    down: bool,
}

impl MemoryTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an open bead of the root with `labels` in `epic`; returns its id (`b-<n>`).
    pub fn add(&self, labels: &[&str], epic: Option<&str>) -> String {
        self.add_in("b", labels, epic)
    }

    /// Adds an open bead with the id prefix `prefix` (a rig's, for a bead of that rig); returns its id
    /// (`<prefix>-<n>`, `n` counted across prefixes).
    pub fn add_in(&self, prefix: &str, labels: &[&str], epic: Option<&str>) -> String {
        self.lock().push(prefix, None, "", labels, epic, "human")
    }

    pub fn bead(&self, id: &str) -> Option<Bead> {
        self.lock().beads.iter().find(|b| b.id == id).cloned()
    }

    /// A human claims `id` as `who`, outside loop-manager.
    pub fn assign(&self, id: &str, who: &str) {
        if let Some(b) = self.lock().beads.iter_mut().find(|b| b.id == id) {
            b.assignee = Some(who.to_owned());
            b.status = "in_progress".to_owned();
        }
    }

    /// A human adds `label` to `id`, as `br label add` does.
    pub fn label(&self, id: &str, label: &str) {
        self.edit(id, &["labels"], |b| b.labels.push(label.to_owned()));
    }

    /// A human retitles `id`, as `br update --title` does.
    pub fn retitle(&self, id: &str, title: &str) {
        self.edit(id, &["fields"], |b| title.clone_into(&mut b.title));
    }

    /// A write of a human to `id` that changes `changed`.
    fn edit(&self, id: &str, changed: &[&str], f: impl FnOnce(&mut Bead)) {
        let mut state = self.lock();
        if let Some(b) = state.beads.iter_mut().find(|b| b.id == id) {
            f(b);
            state.record(id, "updated", "human", changed);
        }
    }

    /// Deletes `id`, as a human does with `br delete`; its children keep it as their parent.
    pub fn remove(&self, id: &str) {
        self.lock().beads.retain(|b| b.id != id);
    }

    /// Someone comments on `id`, as `br comments add` does.
    pub fn comment(&self, id: &str, author: &str, text: &str) {
        let comment = Comment {
            author: author.to_owned(),
            created_at: "2026-11-13T00:00:00Z".to_owned(),
            text: text.to_owned(),
        };
        self.lock().comments.push((id.to_owned(), comment));
    }

    /// While down, every call fails as `Unavailable`.
    pub fn set_down(&self, down: bool) {
        self.lock().down = down;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` on the bead `id` unless the tracker is down; a success is logged as `kind` by `actor`.
    fn with<T>(
        &self,
        id: &str,
        (kind, actor): (&str, &str),
        f: impl FnOnce(&mut Bead) -> Result<T, TrackerError>,
    ) -> Result<T, TrackerError> {
        let mut state = self.lock();
        if state.down {
            return Err(TrackerError::Unavailable {
                text: "down".to_owned(),
            });
        }
        let bead = state.beads.iter_mut().find(|b| b.id == id);
        let out = f(bead.ok_or(TrackerError::NotFound)?)?;
        let changed: &[&str] = if kind == "updated" {
            &["status", "assignee"]
        } else {
            &[]
        };
        state.record(id, kind, actor, changed);
        Ok(out)
    }
}

impl State {
    /// Adds an open bead (titled `Task <id>` without `title`) and logs its creation by `actor`; returns its id.
    fn push(
        &mut self,
        prefix: &str,
        title: Option<&str>,
        description: &str,
        labels: &[&str],
        epic: Option<&str>,
        actor: &str,
    ) -> String {
        let id = format!("{prefix}-{}", self.beads.len() + 1);
        self.beads.push(Bead {
            id: id.clone(),
            title: title.map_or_else(|| format!("Task {id}"), str::to_owned),
            description: description.to_owned(),
            status: "open".to_owned(),
            priority: 2,
            labels: labels.iter().map(|l| (*l).to_owned()).collect(),
            assignee: None,
            epic: epic.map(str::to_owned),
            created_at: "2026-11-13T00:00:00Z".to_owned(),
            updated_at: "2026-11-13T00:00:00Z".to_owned(),
            closed_at: None,
        });
        self.record(&id, "created", actor, &[]);
        id
    }

    /// Whether `id` is the parent of a bead not closed.
    fn has_open_children(&self, id: &str) -> bool {
        self.beads
            .iter()
            .any(|c| c.epic.as_deref() == Some(id) && c.status != "closed")
    }

    fn record(&mut self, id: &str, kind: &str, actor: &str, changed: &[&str]) {
        let Some(bead) = self.beads.iter().find(|b| b.id == id).cloned() else {
            return;
        };
        let seq = i64::try_from(self.log.len()).unwrap_or(i64::MAX) + 1;
        self.log.push(Change {
            seq,
            kind: kind.to_owned(),
            actor: actor.to_owned(),
            changed: changed.iter().map(|c| (*c).to_owned()).collect(),
            bead,
        });
    }
}

/// Fails unless `owner` holds `bead`.
fn held(bead: &Bead, owner: &str) -> Result<(), TrackerError> {
    if bead.assignee.as_deref() == Some(owner) {
        Ok(())
    } else {
        Err(TrackerError::OwnerMismatch {
            holder: bead.assignee.clone(),
        })
    }
}

impl Tracker for MemoryTracker {
    fn claim<'a>(&'a self, owner: &'a str, id: &'a str) -> BoxFuture<'a, Result<Bead, TrackerError>> {
        Box::pin(async move {
            self.with(id, ("updated", owner), |b| match &b.assignee {
                Some(holder) if holder != owner => Err(TrackerError::Claimed { holder: holder.clone() }),
                _ if b.status == "closed" => Err(TrackerError::Invalid {
                    text: "closed".to_owned(),
                }),
                _ => {
                    b.assignee = Some(owner.to_owned());
                    b.status = "in_progress".to_owned();
                    Ok(b.clone())
                }
            })
        })
    }

    fn release<'a>(&'a self, owner: &'a str, id: &'a str) -> BoxFuture<'a, Result<(), TrackerError>> {
        Box::pin(async move {
            self.with(id, ("updated", owner), |b| {
                held(b, owner)?;
                b.assignee = None;
                b.status = "open".to_owned();
                Ok(())
            })
        })
    }

    fn close<'a>(&'a self, owner: &'a str, id: &'a str, _reason: &'a str) -> BoxFuture<'a, Result<(), TrackerError>> {
        Box::pin(async move {
            if self.lock().has_open_children(id) {
                return self.release(owner, id).await;
            }
            self.with(id, ("closed", owner), |b| {
                held(b, owner)?;
                b.status = "closed".to_owned();
                b.closed_at = Some(b.updated_at.clone());
                Ok(())
            })
        })
    }

    fn ready(&self) -> BoxFuture<'_, Result<Vec<Bead>, TrackerError>> {
        Box::pin(async move {
            let state = self.lock();
            if state.down {
                return Err(TrackerError::Unavailable {
                    text: "down".to_owned(),
                });
            }
            let open = state
                .beads
                .iter()
                .filter(|b| b.status == "open" && b.assignee.is_none() && !state.has_open_children(&b.id));
            Ok(open.cloned().collect())
        })
    }

    fn under<'a>(&'a self, root: &'a str) -> BoxFuture<'a, Result<Vec<String>, TrackerError>> {
        Box::pin(async move {
            let ready = self.ready().await?;
            let state = self.lock();
            let parent = |id: &String| state.beads.iter().find(|b| &b.id == id)?.epic.clone();
            if !state.beads.iter().any(|b| b.id == root) {
                return Err(TrackerError::NotFound);
            }
            let ids = ready.into_iter().map(|b| b.id);
            Ok(ids
                .filter(|id| std::iter::successors(parent(id), parent).any(|e| e == root))
                .collect())
        })
    }

    fn lineage<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Lineage, TrackerError>> {
        Box::pin(async move {
            let state = self.lock();
            if state.down {
                return Err(TrackerError::Unavailable {
                    text: "down".to_owned(),
                });
            }
            let commented = |id: &str| {
                let bead = state.beads.iter().find(|b| b.id == id)?.clone();
                let comments = state.comments.iter().filter(|(of, _)| of == id);
                let comments = comments.map(|(_, c)| c.clone()).collect();
                Some(Commented { bead, comments })
            };
            let bead = commented(id).ok_or(TrackerError::NotFound)?;
            // Up the parents while the tracker has them; a deleted one ends the line.
            let mut parents: Vec<Commented> = Vec::new();
            let mut next = bead.bead.epic.clone();
            while let Some(parent) = next
                .as_deref()
                .filter(|p| parents.iter().all(|c| c.bead.id != *p) && *p != id)
            {
                let Some(found) = commented(parent) else { break };
                next = found.bead.epic.clone();
                parents.push(found);
            }
            parents.reverse();
            Ok(Lineage { bead, parents })
        })
    }

    fn changes(&self, after: i64) -> BoxFuture<'_, Result<Vec<Change>, TrackerError>> {
        Box::pin(async move {
            let state = self.lock();
            if state.down {
                return Err(TrackerError::Unavailable {
                    text: "down".to_owned(),
                });
            }
            Ok(state.log.iter().filter(|c| c.seq > after).cloned().collect())
        })
    }

    fn create<'a>(
        &'a self,
        actor: &'a str,
        key: &'a str,
        title: &'a str,
        description: &'a str,
    ) -> BoxFuture<'a, Result<Bead, TrackerError>> {
        Box::pin(async move {
            let mut state = self.lock();
            if state.down {
                return Err(TrackerError::Unavailable {
                    text: "down".to_owned(),
                });
            }
            let named = state.beads.iter().find(|b| b.id == description).map(|b| b.id.clone());
            let made = state.keys.iter().find(|(k, _)| k == key).map(|(_, id)| id.clone());
            let id = match made.or(named) {
                Some(id) => id,
                None => {
                    let id = state.push("b", Some(title), description, &[], None, actor);
                    state.keys.push((key.to_owned(), id.clone()));
                    id
                }
            };
            state
                .beads
                .iter()
                .find(|b| b.id == id)
                .cloned()
                .ok_or(TrackerError::NotFound)
        })
    }
}

#[cfg(test)]
mod tests;

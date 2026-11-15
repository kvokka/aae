//! A tracker in memory (contracts 5) for core tests: claims, conditional release and close, the ready set in
//! creation order, creation once per key, and an outage switch. As in br, a parent with open children is not ready
//! and is released, not closed.

use aae_core::{
    BoxFuture,
    tracker::{Bead, Change, Tracker, TrackerError},
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
    down: bool,
}

impl MemoryTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an open bead with `labels` in `epic`; returns its id (`b-<n>`).
    pub fn add(&self, labels: &[&str], epic: Option<&str>) -> String {
        self.lock().push(None, "", labels, epic, "human")
    }

    pub fn bead(&self, id: &str) -> Option<Bead> {
        self.lock().beads.iter().find(|b| b.id == id).cloned()
    }

    /// A human claims `id` as `who`, outside aae.
    pub fn assign(&self, id: &str, who: &str) {
        if let Some(b) = self.lock().beads.iter_mut().find(|b| b.id == id) {
            b.assignee = Some(who.to_owned());
            b.status = "in_progress".to_owned();
        }
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
        title: Option<&str>,
        description: &str,
        labels: &[&str],
        epic: Option<&str>,
        actor: &str,
    ) -> String {
        let id = format!("b-{}", self.beads.len() + 1);
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
            let id = match state.keys.iter().find(|(k, _)| k == key) {
                Some((_, id)) => id.clone(),
                None => {
                    let id = state.push(Some(title), description, &[], None, actor);
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_key_creates_one_bead_whose_parent_is_released_while_a_child_is_open() {
        let t = MemoryTracker::new();
        let root = t.create("aae:r1", "k1", "Ship it", "text").await.unwrap();
        assert_eq!(t.create("aae:r1", "k1", "Other", "").await.unwrap(), root);
        assert_ne!(t.create("aae:r1", "k2", "Two", "").await.unwrap().id, root.id);
        let child = t.add(&[], Some(&root.id));
        t.claim("aae:r1", &root.id).await.unwrap();
        assert!(t.ready().await.unwrap().iter().any(|b| b.id == child));
        t.close("aae:r1", &root.id, "passed").await.unwrap();
        let root = t.bead(&root.id).unwrap();
        assert_eq!((root.status.as_str(), root.assignee), ("open", None));
        assert!(t.ready().await.unwrap().iter().all(|b| b.id != root.id));
    }
}

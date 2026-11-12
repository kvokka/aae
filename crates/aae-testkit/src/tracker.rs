//! A tracker in memory (contracts 5) for core tests: claims, conditional release and close, the ready set in
//! creation order, and an outage switch.

use aae_core::{
    BoxFuture,
    tracker::{Bead, Tracker, TrackerError},
};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Default)]
pub struct MemoryTracker {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    beads: Vec<Bead>,
    down: bool,
}

impl MemoryTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an open bead with `labels` in `epic`; returns its id (`b-<n>`).
    pub fn add(&self, labels: &[&str], epic: Option<&str>) -> String {
        let mut state = self.lock();
        let id = format!("b-{}", state.beads.len() + 1);
        state.beads.push(Bead {
            id: id.clone(),
            title: format!("Task {id}"),
            description: String::new(),
            status: "open".to_owned(),
            priority: 2,
            labels: labels.iter().map(|l| (*l).to_owned()).collect(),
            assignee: None,
            epic: epic.map(str::to_owned),
            created_at: "2026-11-13T00:00:00Z".to_owned(),
            updated_at: "2026-11-13T00:00:00Z".to_owned(),
            closed_at: None,
        });
        id
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

    /// Runs `f` on the bead `id` unless the tracker is down.
    fn with<T>(&self, id: &str, f: impl FnOnce(&mut Bead) -> Result<T, TrackerError>) -> Result<T, TrackerError> {
        let mut state = self.lock();
        if state.down {
            return Err(TrackerError::Unavailable {
                text: "down".to_owned(),
            });
        }
        let bead = state.beads.iter_mut().find(|b| b.id == id);
        f(bead.ok_or(TrackerError::NotFound)?)
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
            self.with(id, |b| match &b.assignee {
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
            self.with(id, |b| {
                held(b, owner)?;
                b.assignee = None;
                b.status = "open".to_owned();
                Ok(())
            })
        })
    }

    fn close<'a>(&'a self, owner: &'a str, id: &'a str, _reason: &'a str) -> BoxFuture<'a, Result<(), TrackerError>> {
        Box::pin(async move {
            self.with(id, |b| {
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
                .filter(|b| b.status == "open" && b.assignee.is_none());
            Ok(open.cloned().collect())
        })
    }
}

//! The br tracker adapter (spec §10.2, docs/design/contracts.md 5): every call is one `br` process (0.7.6) on the
//! project's database named explicitly by `BEADS_DIR`, with the caller's actor; the writes come from br's `events`
//! table, read-only, and a watcher on `.beads/` says when to read them (docs/design/experiments.md, experiment 3).

use aae_core::{
    BoxFuture,
    tracker::{Bead, Change, Tracker, TrackerError},
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The br versions the adapter is tested on, oldest and newest; the daemon warns outside them.
pub const TESTED: [(u32, u32, u32); 2] = [(0, 7, 6), (0, 7, 6)];
/// How long a call (and a read of `events`) waits for br's write lock before it is `Unavailable`.
const LOCK_TIMEOUT_MS: u64 = 10_000;
/// The actor of the daemon's own reads.
const READER: &str = "aae";

/// The tracker of one project: its `.beads` directory.
pub struct Br {
    dir: PathBuf,
}

impl Br {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Runs `br <args> --json` as `actor`; the parsed stdout, or the failure with br's exit code.
    async fn br(&self, actor: &str, args: &[&str]) -> Result<Value, Fail> {
        let out = tokio::process::Command::new("br")
            .args(args)
            .args([
                "--json",
                "--actor",
                actor,
                "--lock-timeout",
                &LOCK_TIMEOUT_MS.to_string(),
            ])
            .env("BEADS_DIR", &self.dir)
            .env("RUST_LOG", "error")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| Fail(None, unavailable(format!("br: {e}"))))?;
        let body: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        let code = out.status.code();
        if code == Some(0) {
            return Ok(body);
        }
        let error = &body["error"];
        let text = error["message"]
            .as_str()
            .map_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_owned(), str::to_owned);
        // By the exit code, never the text (aae-context/03 §3): 2 with `retryable` is a lock timeout, 130 a shutdown,
        // a killed br an unknown outcome the next read settles.
        Err(Fail(
            code,
            match code {
                Some(3) => TrackerError::NotFound,
                Some(2) if error["retryable"] == true => unavailable(text),
                Some(130) | None => unavailable(text),
                Some(c) => TrackerError::Invalid {
                    text: format!("br exit {c}: {text}"),
                },
            },
        ))
    }

    /// The beads `ids` as `br show` has them, in that order.
    async fn show(&self, ids: &[&str]) -> Result<Vec<Bead>, TrackerError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let args: Vec<&str> = ["show"].into_iter().chain(ids.iter().copied()).collect();
        let shown = self.br(READER, &args).await?;
        shown.as_array().into_iter().flatten().map(bead).collect()
    }

    async fn one(&self, id: &str) -> Result<Bead, TrackerError> {
        self.show(&[id]).await?.pop().ok_or(TrackerError::NotFound)
    }

    /// The bead `id` if `owner` holds it.
    async fn held(&self, owner: &str, id: &str) -> Result<Bead, TrackerError> {
        let bead = self.one(id).await?;
        if bead.assignee.as_deref() == Some(owner) {
            Ok(bead)
        } else {
            Err(TrackerError::OwnerMismatch { holder: bead.assignee })
        }
    }

    /// The rows of br's `events` after `after`, read-only, the database closed again at once (a reader left open
    /// stops br's WAL checkpoints). The read holds br's writer lock `.write.lock` shared: br's own SQLite does not
    /// hold the stock WAL index locks, so a stock reader that thinks it is alone resets `beads.db-shm`, and a br
    /// write opening at that moment fails with `SHM changed identity or length` (br's reads are not affected).
    async fn rows(&self, after: i64) -> Result<Vec<Row>, TrackerError> {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(".write.lock"))
            .map_err(|e| unavailable(format!("br write lock: {e}")))?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(LOCK_TIMEOUT_MS);
        // Polled, not waited for: a br write may hold the lock for seconds, longer than an async thread may block.
        loop {
            match lock.try_lock_shared() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(e) => return Err(unavailable(format!("br write lock: {e}"))),
            }
        }
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let db = rusqlite::Connection::open_with_flags(self.dir.join("beads.db"), flags).map_err(sql)?;
        let mut stmt = db
            .prepare("SELECT id, issue_id, event_type, actor FROM events WHERE id > ?1 ORDER BY id")
            .map_err(sql)?;
        let rows = stmt
            .query_map([after], |r| {
                Ok(Row {
                    seq: r.get(0)?,
                    bead: r.get(1)?,
                    kind: r.get(2)?,
                    actor: r.get(3)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)
    }
}

/// A failed br call: its exit code (`None`: br did not run or was killed) and what it means to the core.
struct Fail(Option<i32>, TrackerError);

impl From<Fail> for TrackerError {
    fn from(Fail(_, e): Fail) -> Self {
        e
    }
}

struct Row {
    seq: i64,
    bead: String,
    kind: String,
    actor: String,
}

impl Tracker for Br {
    fn claim<'a>(&'a self, owner: &'a str, id: &'a str) -> BoxFuture<'a, Result<Bead, TrackerError>> {
        Box::pin(async move {
            match self.br(owner, &["update", id, "--claim"]).await {
                Ok(_) => self.one(id).await,
                // Exit 4 is a refused claim; who holds the bead is read from the bead, not the message.
                Err(Fail(Some(4), e)) => match self.one(id).await?.assignee {
                    Some(holder) if holder != owner => Err(TrackerError::Claimed { holder }),
                    _ => Err(e),
                },
                Err(Fail(_, e)) => Err(e),
            }
        })
    }

    fn release<'a>(&'a self, owner: &'a str, id: &'a str) -> BoxFuture<'a, Result<(), TrackerError>> {
        Box::pin(async move {
            // br has no release: an update conditional on the bead not having changed since the owner was read.
            for _ in 0..3 {
                let bead = self.held(owner, id).await?;
                let args = [
                    "update",
                    id,
                    "-s",
                    "open",
                    "--assignee",
                    "",
                    "--if-unchanged",
                    &bead.updated_at,
                ];
                match self.br(owner, &args).await {
                    Err(Fail(Some(6), _)) => {}
                    other => return Ok(other.map(drop)?),
                }
            }
            Err(unavailable(format!("{id} kept changing")))
        })
    }

    fn close<'a>(&'a self, owner: &'a str, id: &'a str, reason: &'a str) -> BoxFuture<'a, Result<(), TrackerError>> {
        Box::pin(async move {
            // shortcut: br 0.7.6 `close` has no `--if-unchanged`, so a claim between this read and the close is
            // overwritten; only the daemon claims (invariant 3), so only a human could; use it once br has it.
            self.held(owner, id).await?;
            Ok(self.br(owner, &["close", id, "-r", reason]).await.map(drop)?)
        })
    }

    fn ready(&self) -> BoxFuture<'_, Result<Vec<Bead>, TrackerError>> {
        Box::pin(async move {
            let ready = self.br(READER, &["ready", "--unassigned", "--limit", "0"]).await?;
            // `ready` leaves out the description and the parent; `show` has both.
            let ids: Vec<&str> = ready
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["id"].as_str())
                .collect();
            match self.show(&ids).await {
                // A bead deleted between the two reads: the next read has the set right.
                Err(TrackerError::NotFound) => Err(unavailable("the ready set changed while read".to_owned())),
                other => other,
            }
        })
    }

    fn changes(&self, after: i64) -> BoxFuture<'_, Result<Vec<Change>, TrackerError>> {
        Box::pin(async move {
            let rows = self.rows(after).await?;
            let mut out: Vec<Change> = Vec::new();
            // One command writes several rows in a row, of one bead and one actor, each kind once: one change.
            let mut groups: Vec<&[Row]> = Vec::new();
            let mut start = 0;
            for (i, row) in rows.iter().enumerate().skip(1) {
                let group = &rows[start..i];
                if row.bead != group[0].bead || row.actor != group[0].actor || group.iter().any(|r| r.kind == row.kind)
                {
                    groups.push(group);
                    start = i;
                }
            }
            groups.extend((start < rows.len()).then(|| &rows[start..]));
            for group in groups {
                let kinds = || group.iter().map(|r| r.kind.as_str());
                let kind = if kinds().any(|k| k == "created") {
                    "created"
                } else if kinds().any(|k| k == "closed") {
                    "closed"
                } else {
                    "updated"
                };
                let mut changed: Vec<String> = kinds().filter_map(field).map(str::to_owned).collect();
                changed.dedup();
                let bead = match self.one(&group[0].bead).await {
                    Err(TrackerError::NotFound) => continue,
                    other => other?,
                };
                out.push(Change {
                    seq: group[group.len() - 1].seq,
                    kind: kind.to_owned(),
                    actor: group[0].actor.clone(),
                    changed: if kind == "updated" { changed } else { Vec::new() },
                    bead,
                });
            }
            Ok(out)
        })
    }
}

/// The bead field an `events` row of br changed.
fn field(kind: &str) -> Option<&'static str> {
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

/// A bead of `br show --json`; its parent is its epic (br has no epic field, §10.2).
fn bead(v: &Value) -> Result<Bead, TrackerError> {
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

fn unavailable(text: String) -> TrackerError {
    TrackerError::Unavailable { text }
}

fn sql(e: rusqlite::Error) -> TrackerError {
    unavailable(format!("br events: {e}"))
}

/// Watches the `.beads` directory (not the file: br replaces `issues.jsonl` by a rename, once per write) and calls
/// `on_change` for each such rename; a lost or extra call costs nothing, the reader goes by its cursor. The watch
/// lasts as long as the returned watcher.
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<notify::RecommendedWatcher, String> {
    use notify::{EventKind, RecursiveMode, Watcher, event::ModifyKind};
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(e) = event
            && matches!(e.kind, EventKind::Modify(ModifyKind::Name(_)) | EventKind::Create(_))
            && e.paths.iter().any(|p| p.ends_with("issues.jsonl"))
        {
            on_change();
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(watcher)
}

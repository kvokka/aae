//! The JSONL event journal (spec §5.3): one line per outbox event, rotated by size, read back from a `seq`.
//! It is a durable outbox consumer: an event leaves the outbox once every consumer has acknowledged it.

use crate::store;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

/// Version of the event envelope (docs/design/contracts.md 1.1).
pub const SCHEMA: u32 = 1;
const CONSUMER: &str = "journal";

pub struct Journal {
    dir: PathBuf,
    max_bytes: u64,
    files: usize,
}

#[derive(Debug)]
pub enum ReadError {
    /// The requested `seq` was rotated away; the oldest one still held is `oldest`.
    Gap {
        oldest: i64,
    },
    Io(io::Error),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl Journal {
    /// The spec's journal: 10 MB per file, 5 files.
    pub fn new(dir: &Path) -> Self {
        Self::with_limits(dir, 10 * 1024 * 1024, 5)
    }

    pub fn with_limits(dir: &Path, max_bytes: u64, files: usize) -> Self {
        Self {
            dir: dir.to_owned(),
            max_bytes,
            files: files.max(1),
        }
    }

    /// `events.jsonl` is the current file, `events.1.jsonl` the one before it, and so on.
    fn path(&self, n: usize) -> PathBuf {
        match n {
            0 => self.dir.join("events.jsonl"),
            n => self.dir.join(format!("events.{n}.jsonl")),
        }
    }

    /// Appends every outbox event after the journal's cursor, then acknowledges them and drops the outbox rows
    /// all consumers have acknowledged. Lines already in the file (written before a crash, not yet acknowledged)
    /// are not written twice.
    pub(crate) fn drain(&self, conn: &mut Connection) -> Result<(), io::Error> {
        let acked: i64 = conn
            .query_row("SELECT seq FROM consumers WHERE name = ?1", [CONSUMER], |r| r.get(0))
            .map_err(io::Error::other)?;
        let cursor = acked.max(last_seq(&self.path(0))?.unwrap_or(0));
        let rows = outbox_after(conn, cursor).map_err(io::Error::other)?;
        let mut last = cursor;
        for (seq, ts_ms, body) in rows {
            let line = envelope(seq, ts_ms, &body).to_string();
            self.append(&line)?;
            last = seq;
        }
        if last > acked {
            store::ack(conn, CONSUMER, last).map_err(io::Error::other)?;
        }
        Ok(())
    }

    fn append(&self, line: &str) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let current = self.path(0);
        let size = fs::metadata(&current).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size + line.len() as u64 + 1 > self.max_bytes {
            self.rotate()?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&current)?;
        file.write_all(format!("{line}\n").as_bytes())?;
        file.sync_data()
    }

    fn rotate(&self) -> io::Result<()> {
        for n in (0..self.files - 1).rev() {
            let from = self.path(n);
            if from.exists() {
                fs::rename(&from, self.path(n + 1))?;
            }
        }
        Ok(())
    }

    /// Events with `seq >= from`, oldest first, at most `limit`. A `from` older than the oldest event still held
    /// is a gap: the reader recovers through the state API instead (spec §5.3).
    pub fn read_from(&self, from: i64, limit: usize) -> Result<Vec<Value>, ReadError> {
        let files: Vec<PathBuf> = (0..self.files)
            .rev()
            .map(|n| self.path(n))
            .filter(|p| p.exists())
            .collect();
        let mut out = Vec::new();
        let mut first = true;
        for path in files {
            for line in BufReader::new(File::open(&path)?).lines() {
                let event: Value = serde_json::from_str(&line?).map_err(io::Error::other)?;
                let seq = event["seq"].as_i64().unwrap_or(0);
                if first {
                    first = false;
                    if from < seq && seq > 1 {
                        return Err(ReadError::Gap { oldest: seq });
                    }
                }
                if seq >= from && out.len() < limit {
                    out.push(event);
                }
            }
        }
        Ok(out)
    }
}

fn outbox_after(conn: &Connection, seq: i64) -> rusqlite::Result<Vec<(i64, i64, String)>> {
    let mut stmt = conn.prepare_cached("SELECT seq, ts_ms, body FROM outbox WHERE seq > ?1 ORDER BY seq")?;
    let rows = stmt.query_map([seq], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

fn envelope(seq: i64, ts_ms: i64, body: &str) -> Value {
    let mut event = json!({ "schema": SCHEMA, "seq": seq, "ts": rfc3339_ms(ts_ms) });
    if let (Some(out), Ok(Value::Object(fields))) = (event.as_object_mut(), serde_json::from_str::<Value>(body)) {
        out.extend(fields);
    }
    event
}

fn last_seq(path: &Path) -> io::Result<Option<i64>> {
    let Ok(file) = File::open(path) else { return Ok(None) };
    let mut last = None;
    for line in BufReader::new(file).lines() {
        if let Ok(event) = serde_json::from_str::<Value>(&line?) {
            last = event["seq"].as_i64().or(last);
        }
    }
    Ok(last)
}

/// `2026-11-12T14:05:00.123Z` from milliseconds since the Unix epoch (days-to-civil, proleptic Gregorian).
pub(crate) fn rfc3339_ms(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s, milli) = (rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{milli:03}Z")
}

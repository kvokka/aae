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
    /// `(written, acked)`: the last `seq` in the files and the last one acknowledged in the state. Unknown until
    /// the first drain, and again after a failed write, so the files are checked once instead of on every drain.
    cursor: Option<(i64, i64)>,
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

    /// `files` counts the current file, so rotation needs at least two.
    pub(crate) fn with_limits(dir: &Path, max_bytes: u64, files: usize) -> Self {
        assert!(files >= 2, "a journal without rotated files would grow without limit");
        Self {
            dir: dir.to_owned(),
            max_bytes,
            files,
            cursor: None,
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
    /// are not written twice. With nothing new in the outbox it does no file I/O.
    pub(crate) fn drain(&mut self, conn: &mut Connection) -> io::Result<()> {
        let result = self.drain_inner(conn);
        if result.is_err() {
            self.cursor = None;
        }
        result
    }

    fn drain_inner(&mut self, conn: &mut Connection) -> io::Result<()> {
        let (mut written, mut acked) = match self.cursor {
            Some(cursor) => cursor,
            None => {
                let acked: i64 = conn
                    .query_row("SELECT seq FROM consumers WHERE name = ?1", [CONSUMER], |r| r.get(0))
                    .map_err(io::Error::other)?;
                (acked.max(self.recover()?.unwrap_or(0)), acked)
            }
        };
        for (seq, ts_ms, body) in outbox_after(conn, written).map_err(io::Error::other)? {
            self.append(&envelope(seq, ts_ms, &body).to_string())?;
            written = seq;
        }
        if written > acked {
            store::ack(conn, CONSUMER, written).map_err(io::Error::other)?;
            acked = written;
        }
        self.cursor = Some((written, acked));
        Ok(())
    }

    /// The last `seq` in the files. A torn last line (a write cut short by a crash or a full disk) is cut off, so
    /// the next line does not glue onto it. Without a current file (a crash right after rotation) the rotated one
    /// holds the last line.
    fn recover(&self) -> io::Result<Option<i64>> {
        let current = self.path(0);
        if let Ok(bytes) = fs::read(&current) {
            let keep = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            if keep < bytes.len() {
                OpenOptions::new().write(true).open(&current)?.set_len(keep as u64)?;
            }
            if let Some(seq) = last_seq(&bytes[..keep]) {
                return Ok(Some(seq));
            }
        }
        Ok(fs::read(self.path(1)).ok().and_then(|bytes| last_seq(&bytes)))
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

pub(crate) fn envelope(seq: i64, ts_ms: i64, body: &str) -> Value {
    let mut event = json!({ "schema": SCHEMA, "seq": seq, "ts": rfc3339_ms(ts_ms) });
    if let (Some(out), Ok(Value::Object(fields))) = (event.as_object_mut(), serde_json::from_str::<Value>(body)) {
        out.extend(fields);
    }
    event
}

/// The `seq` of the last line that parses: a damaged line is skipped, not taken for the end of the file.
fn last_seq(bytes: &[u8]) -> Option<i64> {
    bytes
        .strip_suffix(b"\n")?
        .rsplit(|&b| b == b'\n')
        .find_map(|line| serde_json::from_slice::<Value>(line).ok()?["seq"].as_i64())
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

use super::*;

#[tokio::test(start_paused = true)]
async fn journal_rotates_continues_from_seq_and_reports_a_gap() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let journal_dir = dir.path().join("journal");
    let journal = Journal::with_limits(&journal_dir, 400, 3);
    let r = rig_with(&db, Clock::managed(T0), 0, Duration::ZERO, journal);
    for _ in 0..20 {
        r.core
            .send(
                None,
                Msg::Work {
                    effect_key: "a",
                    dedupe: "x",
                },
            )
            .await
            .unwrap();
    }
    settle().await;
    let mut files: Vec<_> = std::fs::read_dir(&journal_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 3);
    assert!(files.iter().all(|f| std::fs::metadata(f).unwrap().len() <= 400));
    assert_eq!(count(&db, "outbox"), 0, "acknowledged events leave the outbox");

    let journal = Journal::with_limits(&journal_dir, 400, 3);
    let crate::ReadError::Gap { oldest } = journal.read_from(1, 100).unwrap_err() else {
        panic!("expected a gap")
    };
    assert!(oldest > 1);
    let tail = journal.read_from(oldest, 100).unwrap();
    let seqs: Vec<i64> = tail.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, (oldest..=20).collect::<Vec<_>>());
    let last = &journal.read_from(20, 100).unwrap()[0];
    assert_eq!(last["schema"], json!(crate::SCHEMA));
    assert_eq!(last["type"], json!("test.work"));
    assert_eq!(last["ts"], json!("2027-01-15T08:00:00.000Z"));
    assert_eq!(last["data"], json!({}));
    assert!(journal.read_from(21, 100).unwrap().is_empty());
}

#[test]
fn journal_does_not_repeat_lines_written_before_a_crash_and_keeps_rows_for_lagging_consumers() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = open(&dir.path().join("state.db")).unwrap();
    conn.execute("INSERT INTO consumers (name, seq) VALUES ('other', 0)", [])
        .unwrap();
    for _ in 0..3 {
        conn.execute("INSERT INTO outbox (ts_ms, body) VALUES (0, '{\"type\":\"t\"}')", [])
            .unwrap();
    }
    let mut journal = Journal::new(&dir.path().join("journal"));
    journal.drain(&mut conn).unwrap();
    assert_eq!(
        count(&dir.path().join("state.db"), "outbox"),
        3,
        "'other' has not acknowledged"
    );
    // A crash after the write and before the acknowledgement; the restarted journal checks the file.
    conn.execute("UPDATE consumers SET seq = 0 WHERE name = 'journal'", [])
        .unwrap();
    let mut journal = Journal::new(&dir.path().join("journal"));
    journal.drain(&mut conn).unwrap();
    assert_eq!(journal.read_from(0, 100).unwrap().len(), 3);
    crate::store::ack(&mut conn, "other", 3).unwrap();
    assert_eq!(
        count(&dir.path().join("state.db"), "outbox"),
        0,
        "the last acknowledgement drops the rows"
    );
}

#[test]
fn timestamps_are_rfc3339_utc_with_milliseconds() {
    use crate::journal::rfc3339_ms;
    assert_eq!(rfc3339_ms(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(rfc3339_ms(T0), "2027-01-15T08:00:00.000Z");
    assert_eq!(rfc3339_ms(951_782_400_123), "2000-02-29T00:00:00.123Z");
    assert_eq!(rfc3339_ms(-1), "1969-12-31T23:59:59.999Z");
}

/// A state with `n` outbox events (`seq` 1..=n) and no consumer but the journal.
fn outbox(dir: &Path, n: usize) -> rusqlite::Connection {
    let conn = open(&dir.join("state.db")).unwrap();
    for _ in 0..n {
        conn.execute("INSERT INTO outbox (ts_ms, body) VALUES (0, '{\"type\":\"t\"}')", [])
            .unwrap();
    }
    conn
}

fn seqs(journal: &Journal) -> Vec<i64> {
    let events = journal.read_from(0, 100).unwrap();
    events.iter().map(|e| e["seq"].as_i64().unwrap()).collect()
}

#[test]
fn a_drain_with_nothing_new_does_no_file_io() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = outbox(dir.path(), 2);
    let journal_dir = dir.path().join("journal");
    let mut journal = Journal::new(&journal_dir);
    journal.drain(&mut conn).unwrap();
    // A directory in place of the file fails any read or write of it.
    let current = journal_dir.join("events.jsonl");
    std::fs::rename(&current, journal_dir.join("saved")).unwrap();
    std::fs::create_dir(&current).unwrap();
    for _ in 0..3 {
        journal.drain(&mut conn).unwrap();
    }
    conn.execute("INSERT INTO outbox (ts_ms, body) VALUES (0, '{\"type\":\"t\"}')", [])
        .unwrap();
    assert!(journal.drain(&mut conn).is_err(), "a new event does reach the file");
    std::fs::remove_dir(&current).unwrap();
    std::fs::rename(journal_dir.join("saved"), &current).unwrap();
    journal.drain(&mut conn).unwrap();
    assert_eq!(seqs(&journal), [1, 2, 3]);
}

#[test]
fn a_torn_last_line_is_cut_off_before_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = outbox(dir.path(), 3);
    let journal_dir = dir.path().join("journal");
    std::fs::create_dir(&journal_dir).unwrap();
    std::fs::write(
        journal_dir.join("events.jsonl"),
        "{\"schema\":1,\"seq\":1,\"type\":\"t\"}\n{\"schema\":1,\"seq\":2,\"ty",
    )
    .unwrap();
    let mut journal = Journal::new(&journal_dir);
    journal.drain(&mut conn).unwrap();
    assert_eq!(seqs(&journal), [1, 2, 3]);
}

#[test]
fn a_damaged_last_line_does_not_make_the_journal_repeat_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = outbox(dir.path(), 3);
    let journal_dir = dir.path().join("journal");
    std::fs::create_dir(&journal_dir).unwrap();
    let current = journal_dir.join("events.jsonl");
    let written = "{\"schema\":1,\"seq\":1,\"type\":\"t\"}\n{\"schema\":1,\"seq\":2,\"type\":\"t\"}\n#damaged#\n";
    std::fs::write(&current, written).unwrap();
    Journal::new(&journal_dir).drain(&mut conn).unwrap();
    let text = std::fs::read_to_string(&current).unwrap();
    let tail = text
        .strip_prefix(written)
        .expect("the lines already written stay as they are");
    let added: Vec<Value> = tail.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(
        added.iter().map(|e| e["seq"].as_i64().unwrap()).collect::<Vec<_>>(),
        [3]
    );
}

#[test]
fn a_crash_between_rotation_and_write_does_not_repeat_the_rotated_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = outbox(dir.path(), 3);
    let journal_dir = dir.path().join("journal");
    std::fs::create_dir(&journal_dir).unwrap();
    std::fs::write(
        journal_dir.join("events.1.jsonl"),
        "{\"schema\":1,\"seq\":1,\"type\":\"t\"}\n{\"schema\":1,\"seq\":2,\"type\":\"t\"}\n",
    )
    .unwrap();
    let mut journal = Journal::new(&journal_dir);
    journal.drain(&mut conn).unwrap();
    assert_eq!(seqs(&journal), [1, 2, 3]);
    let current = std::fs::read_to_string(journal_dir.join("events.jsonl")).unwrap();
    assert_eq!(current.lines().count(), 1);
}

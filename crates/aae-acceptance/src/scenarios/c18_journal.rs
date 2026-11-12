//! Criterion 18: the journal rotates at 10 MiB and keeps 5 files; a reader continues from any `seq` still held,
//! and a `seq` rotated away is an explicit gap.
//!
//! Writing 50 MiB of real events would take minutes, so the tree starts with a journal history that is full: five
//! files of seqs 1–50, the current one 100 bytes short of the limit. The daemon treats them as already written
//! and journals its own events from seq 51 on; its first line rotates the files and evicts seqs 1–10.

use super::exec_formula;
use crate::{Env, daemon::Daemon, daemon::write};
use aae_front::client::{ClientError, Events};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const LIMIT: usize = 10 * 1024 * 1024;
const FILES: usize = 5;
const LINES: usize = 10;

/// Ten envelopes from `first` on, each padded to a tenth of a file 100 bytes short of the limit.
fn seeded(first: usize) -> String {
    let line = (LIMIT - 100) / LINES;
    (first..first + LINES)
        .map(|seq| {
            let head = format!("{{\"seq\":{seq},\"type\":\"seed\",\"pad\":\"");
            format!("{head}{}\"}}\n", "x".repeat(line - head.len() - 3))
        })
        .collect()
}

async fn next(events: &mut Events) -> Value {
    let event = tokio::time::timeout(Duration::from_secs(20), events.next()).await;
    let event = event.expect("an event in time").unwrap().expect("the stream goes on");
    serde_json::from_str(&event.data).unwrap()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let d = Daemon::start(&env, |root| {
        let journal = root.join("state/aae/journal");
        for n in 0..FILES {
            let name = if n == 0 {
                "events.jsonl".to_owned()
            } else {
                format!("events.{n}.jsonl")
            };
            write(journal.join(name), seeded(1 + (FILES - 1 - n) * LINES));
        }
        exec_formula(root, "ok", "", "\"true\"");
    })
    .await;
    let journal = d.path("state/aae/journal");
    let current = journal.join("events.jsonl");
    let mut runs = 0;
    while std::fs::metadata(&current).map_or(true, |m| m.len() as usize > LIMIT / 2) {
        assert!(runs < 50, "50 runs did not rotate the journal");
        let run = d.create("ok").await;
        d.until_state(&run, "passed", Duration::from_secs(30)).await;
        runs += 1;
    }
    // Enough events after seq 50 for the reads below.
    for _ in 0..2 {
        let run = d.create("ok").await;
        d.until_state(&run, "passed", Duration::from_secs(30)).await;
    }

    let mut names: Vec<_> = std::fs::read_dir(&journal)
        .unwrap()
        .map(|e| e.unwrap())
        .inspect(|e| assert!(e.metadata().unwrap().len() as usize <= LIMIT, "{e:?} over the limit"))
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    names.sort();
    let kept = [
        "events.1.jsonl",
        "events.2.jsonl",
        "events.3.jsonl",
        "events.4.jsonl",
        "events.jsonl",
    ];
    assert_eq!(names, kept, "five files after rotation");

    match d.client.events(Some(5)).await {
        Err(ClientError::Api { status: 410, error }) => {
            assert_eq!((&error["code"], &error["oldest"]), (&json!("gap"), &json!(11)));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("seq 5 was rotated away, yet the stream continued"),
    }
    let mut events = d.client.events(Some(51)).await.unwrap();
    for seq in 51..56 {
        let envelope = next(&mut events).await;
        assert_eq!(envelope["seq"], seq, "the stream continues in order: {envelope}");
        assert_ne!(envelope["type"], "seed");
    }
    let mut again = d.client.events(Some(53)).await.unwrap();
    assert_eq!(next(&mut again).await["seq"], 53, "a reader continues from a held seq");
    format!("rotated after {runs} runs")
}

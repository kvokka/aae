//! Experiment 3 (spec §16): br as an event source — directory watch on `.beads/`, the `events` table, attribution.
//! Usage: `cargo run -p br-events` (needs `br` on PATH; works in a fresh temp directory).

use notify::{Event, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

struct Bench {
    dir: PathBuf,
    rx: Mutex<Receiver<(Instant, Event)>>,
    cursor: i64,
}

impl Bench {
    fn beads(&self) -> PathBuf {
        self.dir.join(".beads")
    }

    /// Runs `br` with an isolated HOME; returns (start, end, stdout).
    fn br(&self, args: &[&str], env: &[(&str, &str)]) -> (Instant, Instant, String) {
        let start = Instant::now();
        let out = Command::new("br")
            .args(args)
            .current_dir(&self.dir)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &self.dir)
            .env("RUST_LOG", "error")
            .env("BEADS_DIR", self.beads())
            .envs(env.iter().copied())
            .output()
            .expect("run br");
        assert!(
            out.status.success(),
            "br {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (start, Instant::now(), String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// File-system events seen within `wait`, grouped as `file kind` → times.
    fn drain(&self, wait: Duration) -> BTreeMap<String, Vec<Instant>> {
        let mut seen = BTreeMap::<String, Vec<Instant>>::new();
        let rx = self.rx.lock().expect("rx");
        while let Ok((t, e)) = rx.recv_timeout(wait) {
            for p in &e.paths {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                seen.entry(format!("{name} {:?}", e.kind)).or_default().push(t);
            }
        }
        seen
    }

    /// Rows of `events` after the cursor; opens and closes the database each time (an open reader blocks WAL checkpoints).
    fn events(&mut self) -> Vec<[String; 6]> {
        let db = Connection::open_with_flags(self.beads().join("beads.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open db");
        let mut q = db
            .prepare("SELECT id, issue_id, event_type, actor, coalesce(agent_name,''), coalesce(harness,'') || '/' || coalesce(model,'') FROM events WHERE id > ?1 ORDER BY id")
            .expect("prepare");
        let rows: Vec<(i64, [String; 6])> = q
            .query_map([self.cursor], |r| {
                let id: i64 = r.get(0)?;
                Ok((
                    id,
                    [id.to_string(), r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?],
                ))
            })
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");
        if let Some((id, _)) = rows.last() {
            self.cursor = *id;
        }
        rows.into_iter().map(|(_, r)| r).collect()
    }
}

fn is_jsonl_signal(key: &str) -> bool {
    key.starts_with("issues.jsonl ") && (key.contains("Name(") || key.contains("Create("))
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

fn cpu_ms() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let f: Vec<&str> = s[s.rfind(')').map_or(0, |i| i + 2)..].split(' ').collect();
    let ticks: f64 = f[11].parse::<f64>().unwrap_or(0.0) + f[12].parse::<f64>().unwrap_or(0.0);
    ticks * 10.0 // USER_HZ = 100
}

fn main() {
    let dir = std::env::temp_dir().join(format!("aae-exp-br-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let (tx, rx) = channel();
    let mut b = Bench {
        dir: dir.clone(),
        rx: Mutex::new(rx),
        cursor: 0,
    };
    b.br(&["init"], &[]);
    let mut watcher = notify::recommended_watcher(move |e: notify::Result<Event>| {
        if let Ok(e) = e {
            let _ = tx.send((Instant::now(), e));
        }
    })
    .expect("watcher");
    watcher
        .watch(&b.beads(), RecursiveMode::NonRecursive)
        .expect("watch .beads");
    let cpu0 = cpu_ms();

    // 1. Sequential creates: one signal per mutating command? Where does it fall relative to the command?
    let mut ids = Vec::new();
    let (mut before_exit, mut lag) = (Vec::new(), Vec::new());
    let mut per_cmd = BTreeMap::<usize, usize>::new();
    for i in 0..20 {
        let (start, end, out) = b.br(&["create", &format!("bead {i}"), "--json"], &[]);
        ids.push(
            serde_json::from_str::<serde_json::Value>(&out).expect("json")["id"]
                .as_str()
                .expect("id")
                .to_owned(),
        );
        let fs = b.drain(Duration::from_millis(400));
        if i == 0 {
            println!(
                "jsonl signal kinds: {:?}",
                fs.keys().filter(|k| is_jsonl_signal(k)).collect::<Vec<_>>()
            );
        }
        let signals: Vec<Instant> = fs
            .into_iter()
            .filter(|(k, _)| is_jsonl_signal(k))
            .flat_map(|(_, v)| v)
            .collect();
        *per_cmd.entry(signals.len()).or_default() += 1;
        for s in signals {
            lag.push(ms(s.saturating_duration_since(start)));
            before_exit.push(s <= end);
        }
    }
    lag.sort_by(f64::total_cmp);
    println!(
        "sequential creates: signals per command {per_cmd:?}; signal after command start, ms: min {} median {} max {}; before command exit: {}/{}",
        lag[0],
        lag[lag.len() / 2],
        lag[lag.len() - 1],
        before_exit.iter().filter(|x| **x).count(),
        before_exit.len()
    );
    let created = b.events();
    println!("events rows for 20 creates: {}", created.len());

    // 2. Reads must not signal.
    for _ in 0..3 {
        b.br(&["list", "--json"], &[]);
        b.br(&["show", &ids[0], "--json"], &[]);
        b.br(&["ready", "--json"], &[]);
    }
    let reads = b.drain(Duration::from_millis(500));
    println!(
        "9 read commands: jsonl signals {}; other fs events {:?}",
        reads
            .iter()
            .filter(|(k, _)| is_jsonl_signal(k))
            .map(|(_, v)| v.len())
            .sum::<usize>(),
        reads
            .iter()
            .filter(|(k, _)| !is_jsonl_signal(k))
            .map(|(k, v)| format!("{k}×{}", v.len()))
            .collect::<Vec<_>>()
    );

    // 3. Burst: 16 concurrent claims by distinct actors.
    let t = Instant::now();
    std::thread::scope(|s| {
        for (i, id) in ids.iter().take(16).enumerate() {
            let b = &b;
            s.spawn(move || {
                b.br(
                    &[
                        "update",
                        id,
                        "--claim",
                        "--actor",
                        &format!("w{i}"),
                        "--lock-timeout",
                        "60000",
                    ],
                    &[],
                )
            });
        }
    });
    let burst_ms = ms(t.elapsed());
    let burst = b.drain(Duration::from_millis(1500));
    let signals: usize = burst
        .iter()
        .filter(|(k, _)| is_jsonl_signal(k))
        .map(|(_, v)| v.len())
        .sum();
    let rows = b.events();
    let mut by_type = BTreeMap::<String, usize>::new();
    for r in &rows {
        *by_type.entry(r[2].clone()).or_default() += 1;
    }
    let attributed = rows
        .iter()
        .filter(|r| {
            r[3].strip_prefix('w')
                .and_then(|n| n.parse::<usize>().ok())
                .is_some_and(|n| ids[n] == r[1])
        })
        .count();
    println!(
        "16 concurrent claims in {burst_ms} ms: jsonl signals {signals}; events rows {} {by_type:?}; rows whose actor matches the claimer: {attributed}",
        rows.len()
    );

    // 4. Attribution: flag versus environment variables.
    type Case<'a> = (&'a str, Vec<&'a str>, Vec<(&'a str, &'a str)>);
    let cases: [Case; 4] = [
        ("--actor A", vec!["--actor", "A"], vec![]),
        ("BD_ACTOR=B", vec![], vec![("BD_ACTOR", "B")]),
        ("BR_ACTOR=B2", vec![], vec![("BR_ACTOR", "B2")]),
        (
            "BR_AGENT_NAME=C BR_HARNESS=claude BR_MODEL=haiku",
            vec![],
            vec![("BR_AGENT_NAME", "C"), ("BR_HARNESS", "claude"), ("BR_MODEL", "haiku")],
        ),
    ];
    for (n, (label, flags, env)) in cases.iter().enumerate() {
        let id = &ids[16 + n];
        b.br(
            &[&["update", id.as_str(), "--add-label", "probe"][..], flags].concat(),
            env,
        );
        for r in b.events() {
            println!(
                "  {label}: event {} actor={:?} agent_name={:?} harness/model={:?}",
                r[2], r[3], r[4], r[5]
            );
        }
    }
    let _ = b.drain(Duration::from_millis(300));

    println!("watcher CPU of this process over steps 1–4: {} ms", cpu_ms() - cpu0);

    // 5. Cost: event read (open, query, close) versus a full snapshot.
    let t = Instant::now();
    for _ in 0..100 {
        b.cursor = 0;
        b.events();
    }
    let read_ms = ms(t.elapsed()) / 100.0;
    let (s, e, _) = b.br(&["list", "--all", "--json"], &[]);
    println!(
        "events read incl. open/close: {read_ms:.2} ms (all rows); br list --all --json snapshot: {} ms at 20 beads",
        ms(e - s)
    );
    drop(watcher);
    let _ = std::fs::remove_dir_all(Path::new(&dir));
}

//! The acceptance spec of aae (spec §9.2, §15): executable scenarios, one per criterion, on a real `aae daemon`
//! and real binaries, and the checks of the rows that live in cargo and the CI scripts. `aae-acceptance` runs all of
//! them; `aae-acceptance --ci` runs the subset without a model, which CI runs on every commit. The result is one
//! report with a line for every row of §15, run or not, the date, the version of `aae` and the versions of the
//! harnesses.
//!
//! Usage: `aae-acceptance [--ci] [--aae <binary>] [--report <file>] [<criterion>...]`; `aae-acceptance --mcp-probe
//! <log>` is the test MCP service of criterion 14, as aae starts a service; `aae-acceptance --mcp-stdio-probe <log>`
//! is its stdio MCP server, as aae starts one behind its bridge.

mod checks;
mod daemon;
mod scenarios;

use std::{
    fmt::Write as _,
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::{Command, ExitCode},
    sync::Arc,
    time::{Duration, Instant},
};

/// Where a scenario runs (the column "Launch" of spec §15).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Launch {
    /// Without a model: in every spec run and in CI.
    Ci,
    /// With a real model: only when the user runs the whole spec.
    Request,
    /// The release builds: in a whole spec run and in the release workflow on a tag, not in CI on every commit.
    Release,
}

pub(crate) type Notes = Pin<Box<dyn Future<Output = String> + Send>>;

/// One criterion of spec §15. `run` panics on a failed check and returns notes for the report.
pub(crate) struct Scenario {
    pub(crate) criterion: &'static str,
    pub(crate) title: &'static str,
    pub(crate) launch: Launch,
    pub(crate) run: fn(Arc<Env>) -> Notes,
}

/// What every scenario gets: the binary under test and the facts the report stamps measurements with.
pub(crate) struct Env {
    pub(crate) aae: PathBuf,
    pub(crate) date: String,
    pub(crate) version: String,
    pub(crate) host: String,
    /// The cargo target directory the binary runs from: the checks build there.
    pub(crate) target: PathBuf,
}

/// A scenario that has not finished within this is failed.
const SCENARIO_LIMIT: Duration = Duration::from_secs(20 * 60);

struct Args {
    ci: bool,
    aae: PathBuf,
    report: PathBuf,
    target: PathBuf,
    only: Vec<String>,
    /// The probe flag given and its log.
    probe: Option<(String, PathBuf)>,
}

fn args() -> Result<Args, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no directory of the executable")?;
    let target = dir.parent().unwrap_or(dir).to_owned();
    let mut args = Args {
        ci: false,
        aae: dir.join("aae"),
        report: target.join("acceptance/report.md"),
        target,
        only: Vec::new(),
        probe: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ci" => args.ci = true,
            "--aae" => args.aae = it.next().ok_or("--aae needs a path")?.into(),
            "--report" => args.report = it.next().ok_or("--report needs a path")?.into(),
            "--mcp-probe" | "--mcp-stdio-probe" => {
                let log = it.next().ok_or(format!("{arg} needs a path"))?;
                args.probe = Some((arg, log.into()));
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            criterion => args.only.push(criterion.to_owned()),
        }
    }
    Ok(args)
}

/// The first line of `<binary> --version`, or `absent`.
fn version_of(binary: &str) -> String {
    let out = Command::new(binary).arg("--version").output();
    out.ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(str::to_owned))
        .unwrap_or_else(|| "absent".into())
}

fn shell(command: &str) -> String {
    let out = Command::new("sh").args(["-c", command]).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The test MCP service on `AAE_SERVICE_PORT` of 127.0.0.1 with `AAE_SERVICE_TOKEN`, or with `--mcp-stdio-probe`
/// the stdio server, logging to `log`.
async fn probe(flag: &str, log: &Path) -> ExitCode {
    let served = if flag == "--mcp-stdio-probe" {
        aae_testkit::mcp::serve_stdio(log)
    } else {
        aae_testkit::mcp::serve_service(log).await
    };
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aae-acceptance {flag}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn panic_text(error: tokio::task::JoinError) -> String {
    match error.try_into_panic() {
        Ok(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "panic".into()),
        Err(error) => error.to_string(),
    }
}

/// Runs one scenario under the time limit: its notes or why it failed, and its wall time.
async fn execute(env: Arc<Env>, run: fn(Arc<Env>) -> Notes) -> (Result<String, String>, Duration) {
    let at = Instant::now();
    let done = tokio::time::timeout(SCENARIO_LIMIT, tokio::spawn(run(env))).await;
    let result = match done {
        Ok(Ok(notes)) => Ok(notes),
        Ok(Err(e)) => Err(panic_text(e)),
        Err(_) => Err(format!("did not finish within {SCENARIO_LIMIT:?}")),
    };
    (result, at.elapsed())
}

/// The place of a criterion in §15: "4a" after "4" and before "5a".
fn order(criterion: &str) -> (u32, &str) {
    let digits = criterion.find(|c: char| !c.is_ascii_digit()).unwrap_or(criterion.len());
    (criterion[..digits].parse().unwrap_or(u32::MAX), &criterion[digits..])
}

/// A line of the report: `mark` is `pass`, `FAIL`, `not run` (a request or release row of a CI run) or `open` (nothing to run).
struct Row {
    criterion: &'static str,
    title: &'static str,
    mark: &'static str,
    notes: String,
    took: Duration,
}

impl Row {
    fn of(s: &Scenario, (result, took): (Result<String, String>, Duration)) -> Row {
        let (mark, notes) = match result {
            Ok(notes) => ("pass", notes),
            Err(why) => ("FAIL", why),
        };
        Row {
            criterion: s.criterion,
            title: s.title,
            mark,
            notes,
            took,
        }
    }

    fn line(&self) -> String {
        let notes = self.notes.replace('\n', " ").replace('|', "\\|");
        let (c, t, m) = (self.criterion, self.title, self.mark);
        format!("| {c} | {t} | {m} | {:.1} s | {notes} |\n", self.took.as_secs_f64())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("aae-acceptance: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some((flag, log)) = &args.probe {
        return probe(flag, log).await;
    }
    let aae = args.aae.to_string_lossy().into_owned();
    let env = Arc::new(Env {
        version: version_of(&aae),
        date: shell("date -u +%Y-%m-%d"),
        host: shell("uname -sm"),
        aae: args.aae.clone(),
        target: args.target.clone(),
    });
    let wanted = |c: &str| args.only.is_empty() || args.only.iter().any(|o| o == c);
    let pick = |list: &'static [Scenario]| -> (Vec<&'static Scenario>, Vec<&'static Scenario>) {
        let list = list.iter().filter(|s| wanted(s.criterion));
        list.partition(|s| !args.ci || s.launch == Launch::Ci)
    };
    let (checks, mut skipped) = pick(checks::ALL);
    let (chosen, request) = pick(scenarios::ALL);
    skipped.extend(request);
    let started = Instant::now();
    let mut rows = Vec::new();
    let tasks: Vec<_> = chosen
        .iter()
        .map(|s| tokio::spawn(execute(env.clone(), s.run)))
        .collect();
    for (s, task) in chosen.iter().zip(tasks) {
        let row = Row::of(s, task.await.unwrap_or_else(|e| (Err(panic_text(e)), Duration::ZERO)));
        print!("{}", row.line());
        rows.push(row);
    }
    // The checks after the scenarios and one at a time: their compilers would load the host under the timed
    // scenarios, and the test build of `aae` rewrites the binary their daemons run.
    for s in checks {
        let row = Row::of(s, execute(env.clone(), s.run).await);
        print!("{}", row.line());
        rows.push(row);
    }
    for s in skipped {
        let notes = if s.launch == Launch::Release {
            "release builds: run on a tag, or the spec without --ci"
        } else {
            "needs a real model: run the spec without --ci"
        }
        .to_owned();
        rows.push(Row {
            criterion: s.criterion,
            title: s.title,
            mark: "not run",
            notes,
            took: Duration::ZERO,
        });
    }
    for (criterion, title, why) in checks::OPEN.iter().filter(|(c, ..)| wanted(c)) {
        rows.push(Row {
            criterion,
            title,
            mark: "open",
            notes: (*why).to_owned(),
            took: Duration::ZERO,
        });
    }
    rows.sort_by(|a, b| order(a.criterion).cmp(&order(b.criterion)));
    let mut open: Vec<&str> = rows
        .iter()
        .filter(|r| r.criterion != "22" && matches!(r.mark, "FAIL" | "open"))
        .map(|r| r.criterion)
        .collect();
    open.dedup();
    if let Some(row) = rows.iter_mut().find(|r| r.criterion == "22" && r.mark == "pass") {
        let readiness = fs::read_to_string(checks::root().join("docs/READINESS.md")).unwrap_or_default();
        if let Err(why) = checks::readiness_verdict(&readiness, &open) {
            (row.mark, row.notes) = ("FAIL", why);
            print!("{}", row.line());
        }
    }
    let mut report = format!(
        "# aae acceptance spec\n\n- date: {}\n- aae: {}\n- claude: {}\n- codex: {}\n- br: {}\n- host: {}\n- run: {}\n\n\
         <!-- {} id=acceptance-times date={} ver={} env={} src=aae-acceptance note=\"wall time of each row; the scenarios run concurrently, \
         then the checks one by one\" -->\n\
         | criterion | check | result | time | notes |\n|---|---|---|---|---|\n",
        env.date,
        env.version,
        version_of("claude"),
        version_of("codex"),
        version_of("br"),
        env.host,
        if args.ci { "CI subset" } else { "all" },
        scenarios::MARK,
        env.date,
        env.version.replace(' ', "-"),
        env.host.replace(' ', "-"),
    );
    let count = |mark: &str| rows.iter().filter(|r| r.mark == mark).count();
    let (passed, failed, opened) = (count("pass"), count("FAIL"), count("open"));
    for row in &rows {
        report.push_str(&row.line());
    }
    let _ = writeln!(
        report,
        "\n{passed} of {} rows passed, {failed} failed, {opened} open, {} not run, in {:.0} s.",
        rows.len(),
        count("not run"),
        started.elapsed().as_secs_f64()
    );
    let written = args
        .report
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&args.report, &report));
    if let Err(e) = written {
        eprintln!("aae-acceptance: the report {}: {e}", args.report.display());
        return ExitCode::FAILURE;
    }
    println!("{}", report.lines().last().unwrap_or_default());
    println!("report: {}", args.report.display());
    // An open row holds back a run of the whole spec; a CI run does not run the request rows it belongs to.
    if failed == 0 && (args.ci || opened == 0) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

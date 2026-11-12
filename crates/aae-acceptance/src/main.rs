//! The acceptance spec of aae (spec §9.2, §15): executable scenarios, one per criterion, on a real `aae daemon`
//! and real binaries. `aae-acceptance` runs all of them; `aae-acceptance --ci` runs the subset without a model,
//! which CI runs on every commit. The result is a report with the date, the version of `aae` and the versions of
//! the harnesses.
//!
//! Usage: `aae-acceptance [--ci] [--aae <binary>] [--report <file>] [<criterion>...]`.

mod daemon;
mod scenarios;

use std::{
    fmt::Write as _,
    fs,
    future::Future,
    path::PathBuf,
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
}

/// A scenario that has not finished within this is failed.
const SCENARIO_LIMIT: Duration = Duration::from_secs(20 * 60);

struct Args {
    ci: bool,
    aae: PathBuf,
    report: PathBuf,
    only: Vec<String>,
}

fn args() -> Result<Args, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no directory of the executable")?;
    let mut args = Args {
        ci: false,
        aae: dir.join("aae"),
        report: dir.parent().unwrap_or(dir).join("acceptance/report.md"),
        only: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ci" => args.ci = true,
            "--aae" => args.aae = it.next().ok_or("--aae needs a path")?.into(),
            "--report" => args.report = it.next().ok_or("--report needs a path")?.into(),
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

#[tokio::main]
async fn main() -> ExitCode {
    let args = match args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("aae-acceptance: {e}");
            return ExitCode::from(2);
        }
    };
    let aae = args.aae.to_string_lossy().into_owned();
    let env = Arc::new(Env {
        version: version_of(&aae),
        date: shell("date -u +%Y-%m-%d"),
        host: shell("uname -sm"),
        aae: args.aae.clone(),
    });
    let chosen: Vec<&Scenario> = scenarios::ALL
        .iter()
        .filter(|s| !args.ci || s.launch == Launch::Ci)
        .filter(|s| args.only.is_empty() || args.only.iter().any(|c| c == s.criterion))
        .collect();
    let started = Instant::now();
    let tasks: Vec<_> = chosen
        .iter()
        .map(|s| {
            let (env, run) = (env.clone(), s.run);
            tokio::spawn(async move {
                let at = Instant::now();
                let done = tokio::time::timeout(SCENARIO_LIMIT, tokio::spawn(run(env))).await;
                let result = match done {
                    Ok(Ok(notes)) => Ok(notes),
                    Ok(Err(e)) => Err(panic_text(e)),
                    Err(_) => Err(format!("did not finish within {SCENARIO_LIMIT:?}")),
                };
                (result, at.elapsed())
            })
        })
        .collect();
    let mut report = format!(
        "# aae acceptance spec\n\n- date: {}\n- aae: {}\n- claude: {}\n- codex: {}\n- br: {}\n- host: {}\n- run: {}\n\n\
         <!-- {} id=acceptance-times date={} ver={} env={} src=aae-acceptance note=\"wall time of each scenario, \
         scenarios run concurrently\" -->\n\
         | criterion | scenario | result | time | notes |\n|---|---|---|---|---|\n",
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
    let mut failed = 0;
    for (scenario, task) in chosen.iter().zip(tasks) {
        let (result, took) = task.await.unwrap_or_else(|e| (Err(panic_text(e)), Duration::ZERO));
        let (mark, notes) = match result {
            Ok(notes) => ("pass", notes),
            Err(why) => {
                failed += 1;
                ("FAIL", why)
            }
        };
        let notes = notes.replace('\n', " ").replace('|', "\\|");
        let line = format!(
            "| {} | {} | {mark} | {:.1} s | {notes} |\n",
            scenario.criterion,
            scenario.title,
            took.as_secs_f64()
        );
        print!("{line}");
        report.push_str(&line);
    }
    let _ = writeln!(
        report,
        "\n{} of {} scenarios passed in {:.0} s.",
        chosen.len() - failed,
        chosen.len(),
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
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

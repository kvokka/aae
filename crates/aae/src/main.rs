//! `aae` binary entry point.

mod daemon;

use std::{path::PathBuf, process::ExitCode};

const USAGE: &str = "usage: aae --version\n       aae daemon [--config <file>] [--socket <path>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("aae {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("daemon") => daemon(&args[1..]),
        _ => usage(),
    }
}

fn daemon(args: &[String]) -> ExitCode {
    let (mut config, mut socket) = (None, None);
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--config" => &mut config,
            "--socket" => &mut socket,
            _ => return usage(),
        };
        let Some(value) = args.next() else {
            return usage();
        };
        *slot = Some(PathBuf::from(value));
    }
    let started = daemon::Paths::resolve(config, socket).and_then(|paths| {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {e}"))?;
        runtime.block_on(daemon::run(paths))
    });
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aae daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

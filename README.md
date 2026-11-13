# aae

## Running the daemon

`aae daemon` runs in the foreground. It finds the `claude`, `codex` and `br` binaries through mise (`mise which
<name>`), since a daemon under systemd or launchd has no mise shims on its `PATH`; a harness may name its binary in
the config instead (`[harness.claude] binary = "/path/to/claude"`).

- `aae doctor` checks, without a daemon, what the daemon needs: the config, each harness binary with its source,
  version and login, br and its license, the mode of the state and socket directories, the state and its outbox.
  It exits non-zero on an error; warnings (an untested version, no login file) do not fail it. `--json` prints
  the checks as JSON.
- `aae daemon --service systemd` prints a systemd user unit, `aae daemon --service launchd` a launchd agent, both
  running this binary with the given `--config` and the `PATH` of the shell that printed them. Install the file
  yourself; aae never starts a daemon on its own.
- `aae drain` stops the daemon once no run is in a turn (no run starts meanwhile; `--timeout` seconds, 300 by
  default); `aae restart` does the same, then starts the daemon again in the same process.

## The br tracker and its license

A project with a `<project>/.beads` directory uses [br](https://github.com/Dicklesworthstone/beads_rust) as its
tracker. br is a separate binary you install yourself (this repository pins 0.7.6 in `mise.toml`); aae runs it as a
process and never links, vendors or ships it.

**br is not plain MIT.** Its license is MIT with a rider that grants no rights to OpenAI, Anthropic, their affiliates
or anyone acting on their behalf. Check whether that rider applies to you before you install br. Whether it applies to
Claude or Codex agents running `br` inside an aae session has not been assessed.

## Acceptance spec

The acceptance criteria of the first release are executable scenarios in `crates/aae-acceptance`, each on a real
`aae daemon` in a temporary directory tree. `mise run acceptance-ci` runs the scenarios without a model, as CI does on
every commit (about 11 minutes: one of them watches an idle daemon for 10). `mise run acceptance` runs all of them,
including those that call a real model: they use the cheapest model, cost money and need your harness login. Pass
criterion numbers to run only those: `cargo build -p aae && cargo run -p aae-acceptance -- --ci 3 19`. The report,
with the date and the versions of `aae` and the harnesses, goes to `target/acceptance/report.md`.

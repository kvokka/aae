# aae

aae (Adaptive Agent Executor) is an event-driven orchestrator for coding agents. One daemon runs formulas — an
agent with a prompt, or a command — as runs on your projects, through the agents' own harnesses (Claude Code,
Codex), started by hand, by events, by a schedule or by a task tracker, and decides each run's end by an explicit
finisher and by plugins.

## Documentation

- [Installing aae](docs/guide/install.md)
- [Your first project](docs/guide/first-project.md)
- [Your first formula with an agent](docs/guide/first-formula.md)
- [Writing a plugin](docs/guide/plugin.md)
- Reference: [configuration files](docs/design/config.md), [contracts](docs/design/contracts.md) (events, hooks,
  run states), [HTTP API](docs/api/openapi.json)

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
  default); `aae restart` does the same, then starts the daemon again in the same process. A run that waits for a
  human answer (`requires_action`) counts as in a turn, so a drain with one waits for the answer or the timeout.
  Runs queued during the drain start after the restart. A drain asked during a restart, or the reverse, is
  refused (409).

## The br tracker and its license

A project with a `<project>/.beads` directory uses [br](https://github.com/Dicklesworthstone/beads_rust) as its
tracker. br is a separate binary you install yourself (this repository pins 0.7.6 in `mise.toml`); aae runs it as a
process and never links, vendors or ships it.

At start the daemon writes `.bv/workspace.yaml` next to the orchestrator config (by default
`~/.config/aae/.bv/workspace.yaml`): one repository per project with a tracker, with br's id prefix. Run
[bv](https://github.com/Dicklesworthstone/beads_viewer) there, or with `--workspace <that file>`, to see the beads of
all projects at once (checked with bv 0.25.0).

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

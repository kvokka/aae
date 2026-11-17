# aae

aae (Adaptive Agent Executor) is an event-driven orchestrator for coding agents. One daemon runs formulas — an
agent with a prompt, or a command — as runs on your projects, through the agents' own harnesses (Claude Code,
Codex), started by hand, by events, by a schedule or by a task tracker, and decides each run's end by an explicit
finisher and by plugins.

## Documentation

- [aae in one page](docs/overview.md): what it is, the glossary, what the first release leaves out
- [Architecture](docs/architecture.md): the parts, the crates, the life of a run
- [Installing aae](docs/guide/install.md)
- [Your first project](docs/guide/first-project.md)
- [Your first formula with an agent](docs/guide/first-formula.md)
- [Writing a plugin](docs/guide/plugin.md)
- [Demos](demos/README.md): a planner agent files a request as beads, and coder agents on claude and codex do them
- Reference: [configuration files](docs/design/config.md), [contracts](docs/design/contracts.md) (events, hooks,
  run states), [HTTP API](docs/api/openapi.json)
- [Everything else](docs/README.md): the map of the documentation

## Running the daemon

`aae daemon` runs in the foreground. It finds the `claude`, `codex` and `br` binaries through mise (`mise which
<name>`) when mise has them, since a daemon under systemd or launchd has no mise shims on its `PATH`, and otherwise
on its `PATH`; a harness may name its binary in the config instead (`[harness.claude] binary = "/path/to/claude"`).
mise is a convenience, not a requirement: install these binaries any way you like.

- `aae doctor` checks what the daemon needs: the config, each harness binary with its source, version and login, br
  and its license, the mode of the state and socket directories, the state and its outbox. It asks the running
  daemon; before the first start, `aae doctor --config <file>` checks a file with no daemon. It exits non-zero on an
  error; warnings (an untested version, no login file) do not fail it. `--json` prints the checks as JSON.
- Packs and plugins from git, at a tag, a commit or (a pack only) the latest revision, are downloaded by `aae` into
  `~/.cache/aae/git` when it loads the config; `aae cache clear` empties that cache (docs/design/config.md).
- `aae daemon --service systemd` prints a systemd user unit, `aae daemon --service launchd` a launchd agent, both
  running this binary with the given `--config` and the `PATH` of the shell that printed them. Install the file
  yourself; aae never starts a daemon on its own.
- `aae drain` stops the daemon once no run is in a turn (no run starts meanwhile; `--timeout` seconds, 300 by
  default); `aae restart` does the same, then starts the daemon again in the same process. Runs queued during the
  drain start after the restart. A drain asked during a restart, or the reverse, is refused (409).

## The br tracker and its license

A project with a `<project>/.beads` directory uses [br](https://github.com/Dicklesworthstone/beads_rust) as its
tracker. br is a separate binary you install yourself (this repository pins 0.7.6 in `mise.toml`); aae runs it as a
process and never links, vendors or ships it.

At start the daemon writes `.bv/workspace.yaml` next to the orchestrator config (by default
`~/.config/aae/.bv/workspace.yaml`): one repository per project with a tracker, with br's id prefix. Run
[bv](https://github.com/Dicklesworthstone/beads_viewer) there, or with `--workspace <that file>`, to see the beads of
all projects at once (read by bv 0.25.0 on 2026-11-13 at `0e02fda`).
<!-- MEASURE id=bv-workspace-read date=2026-11-13 ver=bv-0.25.0,0e02fda env=linux-arm64-orbstack src="by hand: bv --workspace <file> --robot-next" note="checked once by hand; not in CI; re-check when bv or the file format changes" -->

**br is not plain MIT.** Its license is MIT with a rider that grants no rights to OpenAI, Anthropic, their affiliates
or anyone acting on their behalf. Check whether that rider applies to you before you install br. Whether it applies to
Claude or Codex agents running `br` inside an aae session has not been assessed.

## Development

- `mise run test` runs the unit and integration tests: the local loop.
- `mise run ci` adds formatting, clippy, `cargo deny`, `cargo machete` and the line budgets. It is the gate of a
  change, and all that CI runs on a commit.
- The [acceptance scenarios](docs/acceptance.md) are end-to-end runs on real daemons, some on a real model, with the
  demos and the chaos stand (1000 new seeds a run); they run on request, not on a commit.

mise is optional here too: the tasks are plain commands in `mise.toml`. The tool versions to install without it,
the commands of each task, wall times and the replay of a seed: [docs/guide/development.md](docs/guide/development.md).
The rules for agents working on this repository: [docs/agents](docs/agents/README.md).

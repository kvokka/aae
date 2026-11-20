# Installing loop-manager

loop-manager is one binary: the daemon, the CLI and the bundled addons. It runs on Linux (x86_64, arm64; statically
linked with musl) and macOS (x86_64, arm64). It is checked on Linux arm64 only: the other targets are built and not
exercised ([what the first release leaves out](../overview.md#what-the-first-release-leaves-out)).

## Install a release

Each release on [GitHub Releases](https://github.com/kvokka/loop-manager/releases) carries
`loop-manager-<target>.tar.gz` for the four targets (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`): the binary alone at the root of the archive, with its sha256 in
`loop-manager-<target>.tar.gz.sha256`. Any of these installs it:

```sh
mise use -g github:kvokka/loop-manager   # the archive for this machine, checked against its sha256
cargo binstall loop-manager              # the same archive, through cargo-binstall
cargo install loop-manager --locked      # built from the crates on crates.io; needs Rust 1.100
```

Or download the archive by hand, check it with `sha256sum -c loop-manager-<target>.tar.gz.sha256` (`shasum -a 256 -c`
on macOS), and put `loop-manager` from it on your `PATH`.

## Build it

loop-manager can be built from source. The repository pins its toolchain with [mise](https://mise.jdx.dev):

```sh
git clone <this repository> loop-manager && cd loop-manager
mise install                       # Rust, zig, cargo-zigbuild, br, and claude and codex
cargo build --release --locked -p loop-manager
install -m 0755 target/release/loop-manager ~/.local/bin/loop-manager
```

mise is a convenience, not a requirement. Without it, install Rust 1.100 any way you like (for example with
rustup); `cargo build` then works as above.

`mise run build-all` builds all four release targets with `cargo zigbuild` into
`target/<target>/release/loop-manager`; take the one for your machine (for example `aarch64-unknown-linux-musl` on a
Linux arm64 host). The Linux binaries need no runtime besides the kernel. Without mise, this needs zig 0.17.1,
cargo-zigbuild 0.24.1 and the Rust targets `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
`x86_64-apple-darwin` and `aarch64-apple-darwin`; then run `cargo zigbuild --release --locked --target <target>`
for each.

## What loop-manager runs

loop-manager starts agents through their own command-line harnesses and never downloads or installs them:

- `claude` (Claude Code) and/or `codex` (Codex CLI): the harnesses your agents use. Log in to each once as you
  normally do, or give the agents an API key in `env` (see [the first formula](first-formula.md)). A codex whose
  host config selects a model provider with an `env_key` needs no login: `codex login status` then says "Not logged
  in", and `loop-manager doctor` checks that variable instead.
- `br` (beads_rust): only for projects that keep their tasks in a `.beads` tracker. Read
  [its license](../../README.md#the-br-tracker-and-its-license) before you install it.

Running a harness headless spends the limits of your subscription, and a vendor's terms may restrict how a
subscription is used through its automation interfaces (OpenAI's terms for the codex app-server limit commercial and
hosted use). loop-manager supports API keys for both harnesses; keeping to the vendor's terms is your responsibility.

claude labels every request with `CLAUDE_CODE_ENTRYPOINT` (`sdk-cli` for `claude -p` when it is unset; `cli` is
rewritten to `sdk-cli` there, any other value goes as is). Community A/B runs in
[anthropics/claude-code#97074](https://github.com/anthropics/claude-code/issues/97074), not an official statement, report
that `sdk-cli` spends the 5-hour window of a subscription 1.8–2.5 times faster than another label (2026-11, claude
2.1.3xx); a measurement of this project on a weekly-only limit saw no clear difference (2026-11-18, claude 2.1.327).
<!-- MEASURE id=claude-entrypoint-community date=2026-11-18 ver=claude-2.1.3xx env=community-ab-runs src=anthropics/claude-code#97074 note="not official; several ABBA runs, not every one on cached tool-heavy load saw a difference" -->
<!-- MEASURE id=claude-entrypoint-weekly date=2026-11-18 ver=claude-2.1.327 env=linux-arm64-orbstack-max-20x src="own run, data outside the repository" note="weekly limit only, ratio up to ~1.6, not decisive; measure again on an account with a 5-hour window" --> loop-manager sets no label; set it in an agent's `env`, for
example `CLAUDE_CODE_ENTRYPOINT = "loop-manager"`; whether that suits the vendor's terms is yours to judge. With any
label but `sdk-cli`, claude sends one more request per session: it names the session from the whole first prompt, on
the small model (on the main one behind a third-party `ANTHROPIC_BASE_URL`), uncached and billed, and
`total_cost_usd` does not always count it. The claude adapter skips that request by default.

### Headless session defaults

Every project gets these defaults when loop-manager starts or resumes an agent. Native subagents run under
the same harness settings.

| Harness | Defaults | Override in the agent config |
|---|---|---|
| Claude Code | Sets `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, `DISABLE_TELEMETRY`, `DISABLE_ERROR_REPORTING`, `DISABLE_AUTOUPDATER`, `DISABLE_FEEDBACK_COMMAND` and `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY` to `1`. The feedback switch covers `/bug` too. | Set the variable in `env` or `native.claude.env`. To opt back into telemetry or error reporting, use an empty string. Claude treats any nonempty value for either as an opt-out. |
| Codex app-server | Sets `analytics.enabled = false`, `feedback.enabled = false`, `check_for_update_on_startup = false` and `notify = []`. | Set the key under `native.codex`, for example `native.codex.feedback.enabled = true`. |

Disabling Claude telemetry also disables remote feature-flag fetching. The broader
`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` switch would additionally suppress the check for a lifted rate limit.
Explicit Codex OpenTelemetry exporters use their own settings.
[Claude environment variables](https://code.claude.com/docs/en/env-vars) · [Codex configuration](https://developers.openai.com/codex/config-reference/)

### Interactive sessions

In this repository, `mise exec -- claude` requests experimental Code Mode. Elsewhere, start an interactive
terminal session with `CLAUDE_CODE_EXPERIMENTAL_CODE_MODE=1 claude`, or set that variable in the `env` of
`~/.claude/settings.json`. Claude ignores it in project settings and supports Code Mode only in interactive
terminal sessions on eligible accounts and models. Interactive Codex needs no configuration for loop-manager.
[Claude Code Mode requirements](https://code.claude.com/docs/en/code-mode#requirements)

### Installing the harnesses

Install them any way you like. With mise, the daemon finds the same versions you test with, for example from your
global `~/.config/mise/config.toml`:

```toml
[tools]
claude = "2.1"
codex = "0"
"github:Dicklesworthstone/beads_rust" = "0.7.6"
```

These ranges are what to install (the repository's `mise.toml` takes br at an exact version and claude and codex
by release line). This release supports claude 2.1.329 and codex 0.171.0 and any newer version; below these, or when
`--version` shows no version, `loop-manager doctor` and the daemon print a warning naming the minimum and still run
the harness. br has its own tested range ([tracker-br](../design/tracker-br.md#versions)). The daemon resolves each
binary in this order: `binary` in the config (`[harness.claude] binary = "/path/to/claude"`; harnesses only, br has
no such key), then `mise which <name>`, then `PATH`. A daemon under systemd or launchd has no mise shims on its
`PATH`, which is why mise is asked directly. Without mise, or when mise does not know the tool, the bare name is
looked up on the daemon's `PATH`; `loop-manager doctor` shows which source each binary came from.

## Check the setup

```sh
loop-manager doctor --config ~/.config/loop-manager/config.toml   # before the daemon first runs: this file, from this shell
loop-manager doctor                                      # with the daemon running: as the daemon sees it
```

`loop-manager doctor` checks the config, each harness binary (where it was found, its version, the login), br and its
license, the modes of the state and socket directories, and the state with its outbox. Without `--config` it asks
the running daemon (`GET /v1/doctor`), which makes the checks from inside its process, with its environment, `PATH`
and config file; with no daemon it fails and says so. With `--config <file>` it needs no daemon and checks that file
from the shell it runs in. Both modes print the findings in TOON by default and exit non-zero when a check has
level `error`. Warnings (an unsupported or untested version, no login file) do not fail the command.
`loop-manager doctor --json` prints the API's array of findings. The API returns HTTP 200 even when a finding has
level `error`.

## Directories

loop-manager follows XDG:

| What | Where |
|---|---|
| config | `$XDG_CONFIG_HOME/loop-manager/config.toml` (`~/.config/loop-manager/config.toml`) |
| state (database, journal) | `$XDG_STATE_HOME/loop-manager` (`~/.local/state/loop-manager`) |
| API socket | `$XDG_RUNTIME_DIR/loop-manager/loop-manager.sock` |
| packs and addons from git | `$XDG_CACHE_HOME/loop-manager/git` (`~/.cache/loop-manager/git`); emptied by `loop-manager cache clear` |

## Run the daemon

`loop-manager daemon` runs in the foreground; every other command is a client of its socket. To keep it running, let
your service manager start it. loop-manager prints the service file and never installs or starts one itself:

```sh
loop-manager daemon --service systemd > ~/.config/systemd/user/loop-manager.service
systemctl --user enable --now loop-manager

loop-manager daemon --service launchd > ~/Library/LaunchAgents/dev.loop-manager.daemon.plist
launchctl load -w ~/Library/LaunchAgents/dev.loop-manager.daemon.plist
```

Both files run this binary with the given `--config` and the `PATH` of the shell that printed them.

To stop the daemon, `loop-manager drain` tells it to wait until no run is in a turn (no new run starts meanwhile) and
exit; the command itself returns at once, and `loop-manager status` shows `stopping: drain` until the daemon is gone.
Stopped by a signal (`SIGTERM`, `Ctrl-C`), the daemon does not wait: it ends the processes of its steps and sessions and
exits, and its next start resumes the runs they belonged to. `loop-manager restart` drains the same way and starts it
again in the same process, from the path the daemon was started with: to upgrade, replace the binary at that path (a new
file moved over it is fine), then run `loop-manager restart`. Should the path hold no binary by then, the daemon logs
the error and exits with a failure, for the service manager to see.

Next: [your first project](first-project.md).

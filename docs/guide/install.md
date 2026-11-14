# Installing aae

aae is one binary: the daemon, the CLI and the bundled plugins. It runs on Linux (x86_64, arm64; statically
linked with musl) and macOS (x86_64, arm64).

## Build it

aae is built from source. The repository pins its toolchain with [mise](https://mise.jdx.dev):

```sh
git clone <this repository> aae && cd aae
mise install                       # Rust, zig, cargo-zigbuild and the pinned claude, codex and br
cargo build --release --locked -p aae
install -m 0755 target/release/aae ~/.local/bin/aae
```

`mise run build-all` builds all four release targets with `cargo zigbuild` into
`target/<target>/release/aae`; take the one for your machine (for example `aarch64-unknown-linux-musl` on a Linux
arm64 host). The Linux binaries need no runtime besides the kernel.

## What aae runs

aae starts agents through their own command-line harnesses and never downloads or installs them:

- `claude` (Claude Code) and/or `codex` (Codex CLI): the harnesses your agents use. Log in to each once as you
  normally do, or give the agents an API key in `env` (see [the first formula](first-formula.md)).
- `br` (beads_rust): only for projects that keep their tasks in a `.beads` tracker. Read
  [its license](../../README.md#the-br-tracker-and-its-license) before you install it.

Install them through mise so the daemon finds the same versions you test with, for example in your global
`~/.config/mise/config.toml`:

```toml
[tools]
claude = "2.1"
codex = "0"
"github:Dicklesworthstone/beads_rust" = "0.7.6"
```

These are the version ranges this release is tested against (the repository's `mise.toml`; its tests last ran on
claude 2.1.325 and codex 0.168.0). The daemon resolves each
binary in this order: `binary` in the config (`[harness.claude] binary = "/path/to/claude"`), then
`mise which <name>`, then `PATH`. A daemon under systemd or launchd has no mise shims on its `PATH`, which is why
mise is asked directly.

## Check the setup

```sh
aae doctor
```

`aae doctor` needs no daemon. It checks the config, each harness binary (where it was found, its version, the
login), br and its license, the modes of the state and socket directories, and the state with its outbox. It exits
non-zero on an error; warnings (an untested version, no login file) do not fail it. `aae doctor --json` prints the
same checks as JSON.

## Directories

aae follows XDG:

| What | Where |
|---|---|
| config | `$XDG_CONFIG_HOME/aae/config.toml` (`~/.config/aae/config.toml`) |
| state (database, journal) | `$XDG_STATE_HOME/aae` (`~/.local/state/aae`) |
| API socket | `$XDG_RUNTIME_DIR/aae/aae.sock` |

## Run the daemon

`aae daemon` runs in the foreground; every other command is a client of its socket. To keep it running, let your
service manager start it. aae prints the service file and never installs or starts one itself:

```sh
aae daemon --service systemd > ~/.config/systemd/user/aae.service
systemctl --user enable --now aae

aae daemon --service launchd > ~/Library/LaunchAgents/dev.aae.daemon.plist
launchctl load -w ~/Library/LaunchAgents/dev.aae.daemon.plist
```

Both files run this binary with the given `--config` and the `PATH` of the shell that printed them.

To stop the daemon, `aae drain` waits until no run is in a turn (no new run starts meanwhile) and exits;
`aae restart` does the same and starts it again in the same process, from the path the daemon was started with:
to upgrade, replace the binary at that path (a new file moved over it is fine), then run `aae restart`. Should the
path hold no binary by then, the daemon logs the error and exits with a failure, for the service manager to see.
Next: [your first project](first-project.md).

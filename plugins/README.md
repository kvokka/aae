# loop-manager marketplace

Claude Code and Codex can install either plugin independently from the `loop-manager` marketplace.

| Plugin | What it does |
| --- | --- |
| `loop-manager@loop-manager` | Operator skills for discovering, installing and creating addons, answering waiting runs, recovering failed runs and recapping work. |
| `run-card@loop-manager` | A read-only MCP App that shows the formula, current step, waits and returns for one run. |

## Install

For Claude Code:

```sh
claude plugin marketplace add kvokka/loop-manager
claude plugin install run-card@loop-manager
claude plugin install loop-manager@loop-manager
```

For Codex:

```sh
codex plugin marketplace add kvokka/loop-manager
codex plugin add run-card@loop-manager
codex plugin add loop-manager@loop-manager
```

When installing unpublished changes, use the absolute path to the repository root in the marketplace
command. Run the install command for each plugin you want.

## Find and install addons

With the operator plugin installed, ask “What loop-manager addons are available?” or “Install version X.Y.Z
of this addon for the dev agent in my project.” The addons skill looks up the current catalog and published
versions. You can also give it the Git URL of an addon from another publisher, or ask “Create an addon in
my repository that adds our review checklist to the agent’s prompt.”

## Follow a run

Ask “Show the live card for run_42.” The host calls `show_run_card` once. In a chat client with MCP Apps
support, the expanded card follows the daemon's journal until the run ends. Click its heading to collapse it.
Starting with the second round, `3/4` means the third round out of four allowed. A step with multiple
attempts has its own counter. An icon identifies each step. AI steps show their formula title or agent name.
A recorded outcome summary follows the step name and wraps to fit. In a text client, use `loop-manager run watch run_42` to follow events.

The run-card MCP server requires Python 3.8+ and uses **stdio only** between the host and the plugin.
It reads the local daemon over its Unix socket. By default it follows `LOOP_MANAGER_SOCKET`, then the
CLI's XDG socket locations. For another daemon, pass its absolute socket path in the tool's `socket`
argument, or register its socket once under a name:

```sh
sockets="${XDG_CONFIG_HOME:-$HOME/.config}/loop-manager/sockets"
mkdir -p "$sockets"
ln -s /absolute/path/to/loop-manager.sock "$sockets/work"
```

Ask “Show the work card for run_42” to select that daemon by name. If the default daemon is unavailable and
one registered socket is available, the run ID alone suffices. Explicit socket selections and
`LOOP_MANAGER_SOCKET` are authoritative. An open card follows the selected socket through reconnects.

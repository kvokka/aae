# dcg

An addon of loop-manager for claude and codex agents: every shell command of a session goes through
[`dcg`](https://github.com/Dicklesworthstone/destructive_command_guard) (destructive_command_guard) before it runs, and
a command `dcg` finds destructive (`git reset --hard`, `rm -rf` of a root, `git push --force`, ...) is denied; the
agent sees the reason. The addon brings no rules of its own: what is blocked is what the host's `dcg` and its config
say.

```toml
# the orchestrator's config.toml or a project's loop-manager.toml
[[addon]]
path = "<this directory>"            # or git = "<this repository>", rev = "<tag>", dir = "addons/dcg"

# any settings layer: [settings], a rig, agent.toml, a variant
addons = [{ name = "dcg" }]
```

## What the host needs

- `dcg` on the `PATH` of the sessions: the daemon's `PATH`, or the one a settings layer gives. This repository pins
  version 0.16.0 in `mise.toml`.
- Its config, where `dcg` looks for it: the user's `~/.config/dcg/config.toml` (the `HOME` of the session), a
  `.dcg.toml` at the root of the repository the agent works in, or the file `DCG_CONFIG` names. A claude session and an
  inheriting codex session have the operator's `HOME`; an isolated codex session (`inherit_host = false`) has a `HOME`
  of its own, so give it the host's file: `env.DCG_CONFIG = "${HOME}/.config/dcg/config.toml"` on the same layer.

`dcg install` is not needed: it writes the hook into the operator's own claude settings, while this addon gives it
only to the agents that attach it.

## How it works

The hook `hooks/guard` runs on `PreToolUse` with the matcher `Bash`, which names the shell tool on both harnesses, and
hands the hook input to `dcg`; `dcg` answers in the protocol of the harness, with `permissionDecision: "deny"` and
its reason for a destructive command, and with nothing for any other. It fires inside the subagents of a session too.

When `dcg` is not on the session's `PATH`, the hook exits 2 and says so on stderr: both harnesses block the call, so
no shell command runs unguarded and the agent sees why.

# agent-mail

An addon of loop-manager for claude and codex agents that work on one repository at the same time, each run in its
own Git worktree: through [Agent Mail](https://github.com/Dicklesworthstone/mcp_agent_mail_rust) they write to each
other in a shared thread, reserve the paths they edit, and see what the others hold. A commit of a path another run
holds is refused. One Agent Mail server, started by the daemon, serves every session that attaches the addon.

The addon brings the server launcher, the session's mail identity, a short protocol for the agents, the commit check
and the release of a run's reservations. The project keeps its rigs, formulas, parallelism, CI, merge and publishing.

## What the host needs

- `mcp-agent-mail` on the daemon's `PATH`. The supported and tested version is 0.3.40.
- A loop-manager build with `[[service]].bind` support (included in this release's Git commit).
- `jq`, `curl`, and Git with configured hooks (`git hook list`).
- A session that reaches the server's loopback port. A codex agent with `sandbox_mode = "workspace-write"` may have
  no network; its commits are then refused with "its reservations could not be checked".

## Install

In your project's `loop-manager.toml`, declare the addon and attach it to the project's agents. If you already have a
`[settings]` table, append the attachment to its `addons` array; if the orchestrator already declares the addon, the
attachment alone is enough. No parameter is required.

```toml
[[addon]]
git = "https://github.com/kvokka/loop-manager.git"
rev = "addons/agent-mail/0.2.0"
dir = "addons/agent-mail"

[settings]
addons = [{ name = "agent-mail" }]

[rigs.api]
path = "api"
max_writers = 2
```

The formula of the coding agent works in worktrees, and the drain that hands out the beads runs two children at once:

```toml
# formulas/code.toml
name = "code"
worktree = true

[inputs]
task = { type = "task", required = true }

[[step]]
kind = "ai"
agent = "coder"
```

```toml
# formulas/epic.toml
name = "epic"

[inputs]
task = { type = "task", required = true }

[[step]]
kind = "drain"
formula = "code"
pool = 2
```

`max_writers` counts the writing runs in the rig's shared checkout; a run with `worktree = true` works on its own
branch and is not one of them, so two ready beads overlap. How the branches of the runs are merged and which commit is
accepted is the project's: a formula that checks `HEAD`, a clean tree or a review result works as before. Attach the
addon in `[settings]` of the project rather than on one agent, so that the end of a run whose last step is an `exec`
step releases its reservations too.

Run `loop-manager config check`, then `loop-manager config reload` to apply the change on the project's daemon.

## Parameters

| Parameter | Default | Meaning |
|---|---|---|
| `bind` | `""` | Fixed IPv4 address and port, e.g. `127.0.0.1:8765` or `0.0.0.0:8765`. Empty selects an automatic loopback port |
| `project_key` | `""` | The mail project of the rig: set it on the rig's attachment to keep an existing one. Empty: the canonical path of the rig's repository (the main worktree), or of the rig's directory outside Git |
| `ttl` | `3600` | The seconds a reservation lasts unless renewed, told to the agents |
| `thread` | `""` | The thread of the runs. Empty: one per chain of runs, `<project>.<correlation>` (a character Agent Mail does not take in a thread ID becomes `-`), so the children of one drain share it. Set it to join independent chains |

## How it works

### One server

The daemon starts a shared server when the first agent uses it and stops it five minutes after the last consumer
leaves. Sessions using the same service definition get the endpoint and its bearer automatically. Use the same
`bind` across attachments that share an addon data directory.

### Publish the MCP endpoint from Docker

Set the project's attachment in `loop-manager.toml`:

```toml
[settings]
addons = [{ name = "agent-mail", params = { bind = "0.0.0.0:8765" } }]
```

Publish the container port with `-p 127.0.0.1:8765:8765`. A client on the Docker host can then use
`http://127.0.0.1:8765/mcp/` while the service is running. A fixed address already in use fails the service start.
For a client using a custom DNS name, include that name in Agent Mail’s `HTTP_ALLOWED_HOSTS`.
See [Fixed and automatic service ports](../../docs/design/config.md#fixed-and-automatic-service-ports) for retry behavior.

For an external MCP client, use the current bearer from `runtime/header` in the addon's data directory.
The file contains the full HTTP authorization header. The default directory is
`${XDG_STATE_HOME:-$HOME/.local/state}/loop-manager/addons/agent-mail` **inside the container**.
`runtime/endpoint.json` contains the internal URL and PID. The daemon generates the bearer.

MCP calls require the bearer by default. Agent Mail's explicit `HTTP_ALLOW_LOCALHOST_UNAUTHENTICATED=true`
allows callers the server sees on loopback to omit it. An empty value or `false` requires it. Remote calls
still require it. Agent Mail serves `/health/*` without credentials.

The default tool filter exposes the 12 tools used for coordination. An explicit Agent Mail filter takes precedence.
See the [tool inventory](tools.md) for the full list and how to enable it.

### A session's identity

On `run.preparing` of an `ai` step the addon gives the run's session its project key, thread and TTL; a step of
another kind (`exec`, `drain`, `formula`) gets nothing from the addon. On the session's `SessionStart`,
once the daemon has chosen the workdir and started the server, it registers the run's agent with Agent Mail and keeps
the name Agent Mail gave for `(project, rig, run, agent)` in `agents/` of the data directory: two runs of one agent
get two names, and a later attempt of the run, or a session after the mailbox was lost, registers under the same name
again. The session gets its name, project key, thread and the head of its inbox as context; a subagent works under its
session's name. When the server is not up, the context says so and the session's commits are refused.

### The commit check

On `run.preparing` of an `ai` step in a Git repository the addon puts one named hook, `hook.loop-manager-agent-mail` on `pre-commit`,
into the repository's shared config (the common Git directory), beside the hooks the repository has, those of
`conventional-commits` included. Git runs it for every worktree; it does nothing outside a session that attaches the
addon, so other commits go as usual, with the addon attached or not. In a session it takes the staged paths of the
index being committed, both sides of a rename, and asks the server's `check_file_reservation_conflicts` with the rig's
key and the paths relative to the repository's root, which keeps a worktree under `.git/loop-manager/worktrees/<run>`
from shifting them. A path another agent holds exclusively refuses the commit with the path and its holder; a check
that cannot be made refuses it too. To take the hook out of a repository: `git config --remove-section
hook.loop-manager-agent-mail`.

### The end of a run

On `run.passed`, `run.failed` and `run.cancelled` the addon releases the reservations of the run's mail names, and
only theirs; a second delivery of the event releases nothing more. The end of one turn releases nothing. When the
server is not up, the reservations end with their TTL, and after a lost mailbox the next session registers again and
reserves anew.

## Checks

`crates/loop-manager/tests/e2e/agent_mail.rs` of loop-manager runs the real server with no model:

- `a_session_keeps_its_mail_name_across_a_retry_and_a_lost_mailbox`: the launcher with a clean home, its bearer
  contract and compact tool set, the identity of a run outside Git, a retry, a lost mailbox and a hidden tool;
- `a_codex_agent_gets_the_server_the_session_hook_and_the_data_directory`: the effective config of a codex agent
  that attaches the addon (the session hook answers codex's input as claude's, which the first test feeds it);
- `a_commit_is_checked_on_both_sides_of_a_rename_and_refused_when_it_cannot_be_checked`: real `git commit`s with
  the hook in place: a rename that leaves or makes a path another run holds is refused, a session with no mail
  name, with the server down or with an answer cut short is refused, `HEAD` unchanged, and a commit outside a session goes;
- `two_runs_in_worktrees_coordinate_through_one_server`: two worktree runs share a server, exchange mail, reserve
  files and release their reservations on pass and cancel. On Linux the test verifies that the server exits when
  its daemon is killed.

Run them with `mise exec -- cargo nextest run -p loop-manager -E 'test(agent_mail)'`.

See the [changelog](CHANGELOG.md) for version history.

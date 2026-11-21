# agent-mail

An addon of loop-manager for claude and codex agents that work on one repository at the same time, each run in its
own Git worktree: through [Agent Mail](https://github.com/Dicklesworthstone/mcp_agent_mail_rust) they write to each
other in a shared thread, reserve the paths they edit, and see what the others hold. A commit of a path another run
holds is refused. One Agent Mail server, started by the daemon, serves every session that attaches the addon.

The addon brings the server launcher, the session's mail identity, a short protocol for the agents, the commit check
and the release of a run's reservations. The project keeps its rigs, formulas, parallelism, CI, merge and publishing.

## What the host needs

- `mcp-agent-mail` 0.3.40 or later on the `PATH` of the daemon. It is not part of loop-manager: install it as you
  like. This repository pins 0.3.40 (commit `7d94ecf`) in `mise.toml`; on 0.3.36 a message to a qualified address in
  another project is refused. A mise shim is not enough unless a version is set for the directory the daemon starts
  services in (`/`), for example with `mise use -g`; the launcher names the binary and its error when it does not
  run.
- `jq`, `curl`, and Git with configured hooks (`git hook list`).
- A session that reaches the server's loopback port. A codex agent with `sandbox_mode = "workspace-write"` may have
  no network; its commits are then refused with "its reservations could not be checked".

## Install

In your project's `loop-manager.toml`, declare the addon and attach it to the project's agents. If you already have a
`[settings]` table, append the attachment to its `addons` array; if the orchestrator already declares the addon, the
attachment alone is enough. No parameter is required.

```toml
[[addon]]
path = "/opt/loop-manager/addons/agent-mail"   # or git, rev = "addons/agent-mail/<version>", dir = "addons/agent-mail"

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
| `project_key` | `""` | The mail project of the rig: set it on the rig's attachment to keep an existing one. Empty: the canonical path of the rig's repository (the main worktree), or of the rig's directory outside Git |
| `ttl` | `3600` | The seconds a reservation lasts unless renewed, told to the agents |
| `thread` | `""` | The thread of the runs. Empty: one per chain of runs, `<project>:<correlation>`, so the children of one drain share it. Set it to join independent chains |

## How it works

### One server

The addon declares the service `agent-mail` with `scope = "orchestrator"`: the daemon starts one `mcp-agent-mail
serve` for all the sessions of every project that attaches the addon from the same directory, on a loopback port it
picks, and gives the sessions its MCP endpoint `/mcp/` with its bearer. Nothing of a run, rig or project is in the
service's definition, so no second server starts; a project, rig, run, role and thread reach the server in each call.
The daemon stops the server 5 minutes after the last session that used it.

The launcher `bin/lm-agent-mail serve` keeps Agent Mail's own order of settings, process environment first, then the
user's `config.env`, which it never writes or prints. In the server's process it sets:

- `STORAGE_ROOT` and `DATABASE_URL` to `mailbox/` of the addon's data directory, made before the server starts, so a
  mailbox the user runs elsewhere is never opened by a second server;
- `HTTP_BEARER_TOKEN` to the daemon's token and `HTTP_ALLOW_LOCALHOST_UNAUTHENTICATED=false`, so a request without the
  token is refused even from loopback, whatever the user's file says; the health endpoints `/health/*` answer without
  it, as Agent Mail serves them;
- the host, port and path on the command line.

Every other setting of the user's file applies, the tool filter included: with `TOOLS_FILTER_MODE=exclude` and
`TOOLS_FILTER_CLUSTERS=build_slots` the sessions get 42 of the 45 tools of 0.3.40. When the file sets no `TOOLS_FILTER_`
key, the server offers the 12 tools the protocol uses: `macro_start_session`, `list_agents`, `send_message`,
`reply_message`, `fetch_inbox`, `acknowledge_message`, `file_reservation_paths`, `renew_file_reservations`,
`release_file_reservations`, `check_file_reservation_conflicts`, `request_contact` and `respond_contact`. A tool the
filter hides is named in the error of the addon's operation that needs it. The daemon starts services with `PATH`,
`HOME` and `LANG` only, so the server reads `$HOME/.config/mcp-agent-mail/config.env`.

For its own hooks the launcher keeps the server's URL, process ID and bearer in `runtime/` of the data directory,
readable by the daemon's user only; the bearer never goes on a command line, into a prompt, a report or Git.

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
- `a_commit_is_checked_on_both_sides_of_a_rename_and_refused_when_it_cannot_be_checked`: real `git commit`s with
  the hook in place: a rename that leaves or makes a path another run holds is refused, a session with no mail name
  or with the server down is refused, `HEAD` unchanged, and a commit outside a session goes;
- `two_runs_in_worktrees_coordinate_through_one_server`: a real daemon with a home whose `config.env` sets the
  filter above, anonymous loopback callers and a mailbox of its own; two runs of one claude agent on a fake provider,
  each in its worktree, meet on one server, write and answer in one thread, commit the paths they add, change and
  delete, and the second is refused a commit of the path the first holds, `HEAD` unchanged; the pass of one releases
  its reservations only, the cancel of the other releases the rest. The user's file stays byte for byte.

Run them with `mise exec -- cargo nextest run -p loop-manager -E 'test(agent_mail)'`.

See the [changelog](CHANGELOG.md) for version history.

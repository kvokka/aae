# Your first project

A **project** is a directory with a `loop-manager.toml`. Its runs work in its **root**, the directory of
`loop-manager.toml` unless `[root]` says otherwise, or in one of its **rigs**, further working directories (usually
repository checkouts) a project of several repositories declares. Its own agents and formulas sit next to
`loop-manager.toml`. The orchestrator config lists the projects the daemon serves.

This page sets up a project of one directory, with no rigs, and runs a formula that needs no model. The
[next page](first-formula.md) adds an agent.

## 1. The orchestrator config

`~/.config/loop-manager/config.toml`:

```toml
projects = ["~/work/hello"]

[harness.claude]          # optional: claude is available without it; this sets its defaults
model = "haiku"           # its default model
```

A relative project path is relative to the config file; `~/` is your home directory. Agents may use claude and codex with no
section; a `[harness.<name>]` section only sets a harness's defaults. The full list of keys is in
[the config reference](../design/config.md).

An orchestrator config may also sit elsewhere, for example beside a project's `loop-manager.toml`, and be passed with
`loop-manager daemon --config <file>`. The [demos](../../demos/README.md) do this, so they run on a daemon of their own
and leave `~/.config/loop-manager/config.toml` alone.

## 2. The project

```sh
mkdir -p ~/work/hello/formulas && cd ~/work/hello
git init && echo "Hello" > README.md
```

`~/work/hello/loop-manager.toml`:

```toml
# Nothing is needed: the root is this directory, named `root`. To change either:
# [root]
# name = "main"
# path = "src"            # relative to this file; it must exist and need not be a git repository
```

The project is named after its directory (`hello`); `name = "..."` in `loop-manager.toml` overrides that. A project of
several repositories adds a `[rigs.<name>]` with the `path` of each; the root is never one of them.

## 3. A formula without a model

A **formula** says how a run works: the process, never the particular task, which comes in as the run's inputs. It is a
list of steps, run in order; each is `exec` (a command), `ai` (an agent working through its prompt template),
`drain` (a run for every bead under the run's bead, see [Runs over a tracker](first-formula.md#runs-over-a-tracker)),
`formula` (a child run) or `await` (an outcome from outside); all five are in
[Formula](../design/config.md#formula-formulasfiletoml). This one has a single step.
`~/work/hello/formulas/check.toml`:

```toml
name = "check"
description = "The project has a README"

[[step]]
kind = "exec"
command = ["test", "-f", "README.md"]   # runs in the run's directory: here, the root
timeout = "30s"
```

An `exec` step passes when the command exits 0 and fails otherwise.

## 4. Start the daemon and run it

In one terminal:

```sh
loop-manager daemon
```

In another:

```sh
loop-manager project list                      # the project with its root and no rigs
loop-manager formula list                      # the formula with its inputs
loop-manager run start hello check             # project, formula (no rig: the root); prints the run id, e.g. run_1
loop-manager run list                          # runs, newest first, with their state and step
loop-manager run show run_1                    # the whole run: state, attempts, outcome
```

`loop-manager events` follows the event stream live (`--from <seq>` to replay from a point),
and `loop-manager run watch <run>` follows one run with the raw messages of its session. Structured replies are TOON
by default. Scripts can read JSON with `--json`, for example `loop-manager run list --json | jq '.[].id'`.

## 5. Change the config

The daemon reads the config at start and again only when you ask:

```sh
loop-manager config check      # report the errors of the daemon's files on disk, apply nothing
loop-manager config reload     # apply them; on an error the old config stays in force
```

Loading is all or nothing, every error found is reported with its file, and an unknown key is an error. A file
that does not parse gives its first error only.

## Triggers

A run need not be started by hand. A `[[trigger]]` in `loop-manager.toml` starts a formula on an event, on a cron
schedule (in the system time zone, or the one `timezone` names; see [Trigger](../design/config.md#trigger)) or at an
interval:

```toml
[[trigger]]
name = "hourly-check"
every = "1h"
run = "check"             # with no `rig`: in the root
```

`loop-manager trigger list` shows the triggers with their next firing, and
`loop-manager trigger fire --project hello --rig root hourly-check` fires one by hand; `loop-manager trigger disable`
(with the same arguments) stops it firing until `loop-manager trigger enable`. Next:
[your first formula with an agent](first-formula.md).

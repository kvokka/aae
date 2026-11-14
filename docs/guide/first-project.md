# Your first project

A **project** is a directory with an `aae.toml`. It has one or more **rigs**, the working directories its runs work
in (usually a repository checkout), and its own agents and formulas next to `aae.toml`. The orchestrator config
lists the projects the daemon serves.

This page sets up a project with one rig and runs a formula that needs no model. The
[next page](first-formula.md) adds an agent.

## 1. The orchestrator config

`~/.config/aae/config.toml`:

```toml
projects = ["~/work/hello"]

[harness.claude]          # a harness the agents of every project may use
model = "haiku"           # its default model
```

A relative project path is relative to the config file; `~/` is your home directory. Add `[harness.codex]` to let
agents use codex as well. The full list of keys is in [the config reference](../design/config.md).

## 2. The project

```sh
mkdir -p ~/work/hello/formulas && cd ~/work/hello
git init && echo "Hello" > README.md
```

`~/work/hello/aae.toml`:

```toml
[rigs.main]
path = "."                # the rig's working directory, relative to this file; it must exist
```

The project is named after its directory (`hello`); `name = "..."` in `aae.toml` overrides that.

## 3. A formula without a model

A **formula** says what a run does. Its one step is either `exec` (a command) or `prompt` (an agent working from a
prompt). `~/work/hello/formulas/check.toml`:

```toml
name = "check"
description = "The rig has a README"

[step]
kind = "exec"
command = ["test", "-f", "README.md"]   # runs in the rig's directory
timeout = "30s"
```

An `exec` step passes when the command exits 0 and fails otherwise.

## 4. Start the daemon and run it

In one terminal:

```sh
aae daemon
```

In another:

```sh
aae project list                      # the project with its rig
aae formula list                      # the formula with its inputs
aae run start hello main check        # project, rig, formula; prints the run id, e.g. run_1
aae run list                          # run_1  passed  hello/main  hello.check  ...
aae run show run_1                    # the whole run: state, attempts, outcome
```

`aae events` follows the event stream live (`--from <seq>` to replay from a point), `aae run watch <run>` one run with
the raw messages of its session, and every command takes
`--json` to print the API's JSON as it is.

## 5. Change the config

The daemon reads the config at start and again only when you ask:

```sh
aae config check      # report the errors of the files on disk, apply nothing; needs no daemon
aae config reload     # apply them; on an error the old config stays in force
```

Loading is all or nothing, every error is reported with its file, and an unknown key is an error.

## Triggers

A run need not be started by hand. A `[[trigger]]` in `aae.toml` starts a formula on an event, on a cron schedule
(in the system time zone, or the one `timezone` names; see [Trigger](../design/config.md#trigger)) or at an interval:

```toml
[[trigger]]
name = "hourly-check"
every = "1h"
run = "check"
rig = "main"
```

`aae trigger list` shows the triggers with their next firing, and
`aae trigger fire --project hello --rig main hourly-check` fires one by hand; `aae trigger disable` (with the same
arguments) stops it firing until `aae trigger enable`. Next:
[your first formula with an agent](first-formula.md).

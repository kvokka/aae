# Recovering a failed run by hand

Inside a live run the policy recovers by itself: a failed attempt is resumed or retried in a new session
([default policy](../design/contracts.md#10-default-policy-as-decisions)). A run that has failed for good is yours
to sort out. No recovery fits every failure, so aae gives you the tools and leaves the choice to you: look at what
failed, then continue the session, start a new session on the same task with or without a note, or abandon the
task. The examples use a project `shop` with a formula `fix` whose `task` input takes a bead
([runs over a tracker](first-formula.md#runs-over-a-tracker)).

The skill `recover` of the aae plugin for Claude Code and Codex walks your agent through the same steps: it finds the
failed runs, shows their failure kinds, offers the ways to go on and runs the one you pick. The repository is the
plugin's marketplace:

```sh
claude plugin marketplace add kvokka/aae && claude plugin install aae@aae
codex plugin marketplace add kvokka/aae && codex plugin add aae@aae
```

## See what failed

```sh
aae run list --state failed               # newest first; --project, --limit, --parent <run>, --awaiting narrow it
aae run show <run>                        # or with --json
```

`aae run show` gives the run's `class` (`agent`, `check`, `hollow_done`, `infrastructure`) and `reason`, the `bead`
it worked on, its `workdir`, and its `attempts`: for each one the `kind` (`new_session` or `resume`), the
`failure_kind` ([failure kinds](../design/contracts.md#44-failure-kinds)) and the `session`, the harness's own
reference (claude `{session_id, config_dir, workdir}`, codex `{thread_id, codex_home}`). With it you can read or open
the conversation with the harness's own tools. `aae run list --state failed --json` gives the same view for every run
in the list. A run of a formula with children (a `drain` or `formula` step) lists them with
`aae run list --parent <run>`.

A run that failed has already let go of its bead: the bead is open and unassigned again, and its failure count went up
by one (a passed run resets it). At `bead_failures` failures (default 3) aae closes the gate `bead:<project>/<id>`: no
run takes the bead until you open it (`aae gate list`, `aae gate open bead:shop/<id>`); opening it resets the count.

## Continue the session

A new run continues the last session of the failed one, in the same workdir (a removed worktree is brought back on
its branch):

```sh
aae run start shop web fix --continue-from <run> --input task=<bead>
```

The formula's first step must be `ai`, on the harness of the session; its prompt goes into the resumed session.
Only the first step continues the session; later steps start their own.

## Start a new session on the same task

A new run of the formula on the same bead takes it up again, in a new session:

```sh
aae run start shop web fix --input task=<bead>
```

With `worktree = true` the new run gets a new worktree; the work of the failed run stays on its branch
`aae/<run>` if it was not merged.

## Start a new session with a note

To tell the new session that the work was begun before and its state is unknown, give the formula an input for the
note and put it in the prompt. A prompt template belongs to an agent and is checked against every formula that
names the agent, so the note gets an agent and a formula of its own: `agents/refixer/agent.toml` the same as that of
the agent of `fix`, and `agents/refixer/prompt.template.md`:

```markdown
{{ inputs.note }}

Fix {{ bead.title }}: {{ bead.description }}
```

and `formulas/refix.toml`:

```toml
name = "refix"
[inputs]
task = { type = "task", required = true }
note = { type = "string", required = true }

[[step]]
kind = "ai"
agent = "refixer"
```

```sh
aae run start shop web refix --input task=<bead> \
  --input 'note=This task was started before by run <run>; its state is unknown. Find out what was done, then finish it.'
```

## Abandon it

A run that is not over yet (in `queued`, `parked` or `interrupted`, say) is stopped with
`aae run cancel <run> --reason <text>`: its session stops, its bead is released without counting a failure, and the
runs its sessions started are cancelled too. A run that already failed cannot be cancelled and needs nothing more;
its bead is open again, so a trigger on `bead.ready` or a `next_ready` task may take it. To keep every run off it,
close its gate:

```sh
aae gate close bead:shop/<bead> --reason "abandoned"
```

A run that names the bead then waits `queued` and `next_ready` passes it over, until `aae gate open bead:shop/<bead>`.
To drop the task for good, close the bead in the tracker.

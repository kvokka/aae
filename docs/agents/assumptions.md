# Assumptions and open questions

An assumption is a choice the author of the design or an implementer made where the owner said nothing. It holds
until the owner corrects it. An agent may propose a replacement, and may replace one with a reason, but never
silently: the change comes with a recorded decision (a `DECISION` comment in devloop), an edit of this file and of the product document in the
same change. What the owner decided is in [decisions.md](decisions.md) and is not replaced at all.

The behaviour itself is described in the document named under each heading; this file only marks it as assumed.

## Waiting for the owner

| What | State |
|---|---|
| `${NAME}` in the `env` of an MCP server, a `[[service]]` or an orchestrator hook | not resolved: the values are literal ([config](../design/config.md#environment-values)); chosen by the implementer |

## Product shape — [overview](../overview.md), [event model](../design/event-model.md)

- A local daemon of one user: no Windows, no several machines or users. One daemon per user, projects register in it.
- No ACP hub "up" for editors.
- The orchestrator's state is its own SQLite database.
- Config files are TOML.
- Adapters are modules of the one binary, with no protocol between processes; the outside path later is an ACP v2
  driver.
- One journal and one `seq` counter per orchestrator, for all projects.
- The core does not store the content of a conversation: the live view relays the adapter's stream, the history is
  the harness's transcript.
- After a restart the daemon stops every recorded harness instance and resumes the runs; it does not reconnect.
- The account id of an account gate is reported by the adapter.
- Issue gates are the only mechanism of "do not start for now".

## Config — [config](../design/config.md)

- The root has a section of its own (a name and a path), not a rig among the rigs. With a root path other than the
  directory of `loop-manager.toml`, the config files and the tracker database stay next to `loop-manager.toml`.
- In a rig, `agents.<name>` is either settings or the name of a variant.
- Merging: a scalar is the stronger layer's; a table merges by key with whole entries replaced; a named list is a
  union, an element switched off by name on a stronger layer.
- The host layer and the model default are taken for the resulting harness; the host layer is opaque to the core.
- The harness's own config inside a rig (`CLAUDE.md`, `.claude/`, `.mcp.json`, `AGENTS.md`) works with host
  inheritance off too.
- Config is re-read by an explicit command, not by watching files.
- A harness binary is resolved as: explicit path in the config → `mise which` → `PATH`.
- An `env` reference has one strict form: `${NAME}` and `$$`; any other `$` is a load error, so a shell-style
  `$NAME` does not pass silently as text.
- `${LOOP_MANAGER_ADDON_DIR}` in an `env` value outside an addon's `[agent]` is a reference like any other, so it fails
  the start as `config_error` unless the daemon has that variable.
- Names the owner did not name: the root's section and its keys, the input type `task`, the `native.<harness>`
  block for the permission mode, the way a template takes an agent's file (`{{ agent.<name> }}`).

## Harnesses — [contracts 4](../design/contracts.md#4-driver-interface)

- The driver interface has no steer mid-turn, no change of model inside a live session, no human joining.
- An instance has a limit of sessions; past it a second instance with the same fingerprint starts.

## Agents, packs, formulas — [config](../design/config.md)

- An `ai` step has no prompt file of its own: it takes its agent's template.
- A project's file is substituted into the template by name; a missing one is a config error that names the path.
- The default `scope` is `rig`.
- The root of a project is an implicit local pack; names are unique within a pack, `<pack>.<name>` across packs.
- Packs from git: "the latest revision" is the tip of the default branch, found when the config loads; offline, the
  cached revision with a warning; one XDG cache per user; no lock file; a running run keeps its snapshot. An addon
  from git must be pinned.
- After `soft_stop` or `hard_stop` a `drain` step ends `fail`.
- A check script runs after the step did; the step's own failure is a failed attempt too; the path is relative to
  the formula file; the environment is that of an `exec` step (for a step of another kind, the one of `[exec]` of the
  orchestrator and the project, never the agent's `env`); a repeat of an `ai` step continues
  its session with the script's output as a remark; an infrastructure failure does not spend `max_attempts`.
- The result of a step is visible to the next steps; a failed step without a return ends the run; checks in a row
  all run and their remarks go back together; the executor of a bead is the formula named in the `drain` step; a
  repeat of a step stays within the round.
- A template is substitution only, without logic; a missing variable is an error before the start.
- A prompt names the comments of the run's bead `bead.comments` and its parents `bead.parents`, the root first, each
  parent with every field of `Bead` and its comments; nothing but a prompt template may name them. All comments are
  shown unless the `ai` step has `last_comments`, which keeps the last of the bead and of each parent.
- The bead is read again each time a run enters an `ai` step whose prompt shows its comments or parents; the other
  steps see the bead as it was last read.
- TOON is written by loop-manager's own encoder, the plain forms of spec 4.4 only: the crates that write TOON turn on
  `preserve_order` of `serde_json` for the whole build, which would change the key order of every JSON loop-manager
  writes. An object or array placeholder inside any template text is TOON, in a trigger's `inputs` too.
- What a script reads stays JSON: `LOOP_MANAGER_STEP_<NAME>_DATA`, `LOOP_MANAGER_INPUT_<NAME>`, `--json` of the CLI.
  CLI structured replies default to TOON, as do MCP tools' text results.

## Runs and triggers — [contracts 6](../design/contracts.md#6-run), [config: trigger](../design/config.md#trigger)

- A run without a rig goes in the root; a step of an agent of scope `root` goes in the root whatever the run's rig.
- Concurrency is bounded per orchestrator and per agent.
- A rig's `max_writers` counts a run by its whole formula, not by the step it is at: no worktree and an `exec` step or
  an `ai` step of an agent not of scope `root`. A parked run keeps its place. A run in a `drain` or `formula` step
  lends its place to the runs that step started, down the chain; a run a session starts counts its parent, which
  works in the directory. A queued run past the limit waits for `writers`, a reason of its own. `[root]` has no
  `max_writers`.
- The states from `preparing` to `finishing` are those of the current step.
- Cancelling a parent cancels its live child runs.
- The outcome of a run is that of its last step; the finisher's data is visible to the next steps, triggers and the
  waiting parent.
- The finisher is a CLI command with the attempt token; the finisher of a stale attempt is refused.
- A remark of a `run.finishing` hook is a return within the step, bounded by the ceiling of invariant 16.
- A run started by a trigger inherits the correlation of the event; a human's write in the tracker starts a new
  chain. The depth limit is 10; `bead.ready` adds no depth.
- While a run of the same trigger with the same input key is alive, a new firing is skipped (`trigger.skipped`).
- `bead.ready` is raised when a bead first enters the ready set.

## Tracker — [tracker](../design/tracker-br.md)

- The tracker is a narrow adapter; a project works without one.
- br has no empty prefix, so the id of a root's bead starts with the database's own prefix (the owner agreed for
  now).
- A bead filed by an agent of scope `rig` gets the rig's prefix by itself; an agent of scope `root` files a rig's
  bead with that rig's prefix.
- There is no `rig:` label. Equal prefixes of two rigs are a config error; a rig prefix equal to the database
  prefix is a `doctor` warning.
- The input `task` is the id of an existing bead or a text the root bead is filed from; "the next ready bead" is
  another form of the same input, resolved by the daemon. Choice and claim are one operation; nothing ready means
  no run and an explicit answer.
- The `harness:` label shows the harness, it does not set it; a bead whose label differs from the effective config
  is not given out, and the reason is visible.
- Every run that ended `failed` while it held the claim is a failed run of its bead, of any class, `infrastructure`
  among them: a run fails only when the recovery inside it is spent. After `bead_failed_runs` of them (default 3)
  the core closes that bead's gate until a human opens it.

## Addons and frontends — [contracts 3](../design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts), [MCP](../api/mcp.md)

- A failed veto hook blocks the run; a failed observing hook is ignored.
- Several addons on one event: one block is enough for a veto; for the reaction to a failure the first decision in
  attachment order wins.
- The bundled addons are outside the core's budget, on a line of their own.
- MCP has five tools, and the server is attached to an agent only when the agent's config says so.

## Delivery and process — [install](../guide/install.md), [code rules](rules-code.md)

- Directories follow XDG; the daemon runs in the foreground; a command generates service files; the CLI never
  starts a daemon by itself. Packs and addons from git are downloaded by `loop-manager` outside the core; mise does not
  touch them.
- The budget limits of the parts (adapters, tracker, frontends, addons, the rest) are estimates; the release limit
  and the hard stop the owner confirmed.
- If the budget does not fit, the first to go are: continuing another run's session, the `bv` workspace file.
- The numeric thresholds of the [acceptance criteria](../acceptance.md) were proposed by their author.

## Not verified

Open questions the code must not lean on without an experiment
([experiments](../design/experiments.md) has what was checked):

- The codex sandbox `workspace-write` with the finisher and the tracker database outside the workdir; until then
  the default is `danger-full-access`.
- A codex subscription login in a separate `CODEX_HOME`.
- claude model short names other than `haiku` (`fable`, `opus`, `sonnet`): no test runs them.
- A real quota running out and a login expiring mid-session; the claude half of criterion 10.
- Dozens of parallel sessions, runs of many hours; a cold start on the models of real roles.
- Everything on macOS and on x86_64.
- With host inheritance off: a claude token expiring mid-session.
- br's "refuse rather than lose" in a long unattended loop; whether the rider of br's license applies to Claude and
  Codex agents running `br`.
- Whether claude's `result.usage` counts one turn or the whole process.
- The stop of what a session left running, beyond the fake providers of one version pair: codex 0.167.2 (in the
  tested range, `thread/backgroundTerminals/clean` was run on 0.168.0 only), claude's `stop_task` while a background
  subagent is alive, and whether the MCP servers of a codex thread, which carry the attempt token, die with it.
- opencode altogether.

## Risks the owner accepted

| Risk | What holds it back |
|---|---|
| Agents commit faster than a human checks | the budgets and the "no code for later" rule, checked by `mise run ci`; the review |
| Vendor protocols drift: codex app-server is experimental, harnesses ship weekly | release lines in `mise.toml`, the tested version range of each adapter, contract tests, a narrow subset of the protocol |
| Subscription policy: headless use spends subscription limits, and vendor terms limit commercial and hosted use | API keys are supported; the install guide says keeping to the vendor's terms is the user's responsibility and [what claude's entrypoint label does](../guide/install.md#what-loop-manager-runs) |
| br in a public project: one author, no outside PRs, a license with a rider | br is installed apart; the license warning in the README, the install guide and `doctor`; a narrow tracker interface |
| A daemon's death cuts claude sessions | `--resume`, a drain on a planned restart |
| Vendors close the niche | the rule "check the vendor first" and the stop conditions of the [code rules](rules-code.md#budgets) |
| The core grows by "one more mechanism" | the mechanism and policy boundary, the core's budget, `scripts/core-boundary.sh` |

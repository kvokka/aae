# Owner decisions

What the owner decided. An agent does not revisit a decision: a change needs the owner's word, and a departure found
in the code is a defect, not an option. What is not listed here and is not said by the owner elsewhere is an
implementation choice or an [assumption](assumptions.md).

The numbers are stable: old notes say "decision 49". Missing numbers were assumptions (12, 33, 34) or are retired
(31, 53, 55: the readiness file and the spec are gone). The behaviour itself is described in the document named under
each heading; this file only marks it as decided.

## The shape of the product — [overview](../overview.md), [architecture](../architecture.md)

| № | Decision |
|---|---|
| 1 | The name is `loop-manager`: it supervises the loops agents work in |
| 2 | The core is written in Rust |
| 5 | No tmux: a terminal screen is never a control channel |
| 22 | The event journal is JSONL, rotated at 10 MB × 5 files |
| 28 | A human joining a live session and handover of control are a later release: no operations, states or stubs now |
| 43 | The core, `loop-manager-contract` and `loop-manager-core`, names no harness and no br, and the adapters (`loop-manager-claude`, `loop-manager-codex`, `loop-manager-br`, `loop-manager-addons`) do not depend on `loop-manager-core`; CI checks both |
| 61 | Everything an adapter reports reaches the policy and the observer; the core drops nothing of it |
| 62 | Safety timers are allowed; polling as a way to wait for a unit of work is forbidden |
| 65 | A harness is an abstraction: all knowledge of one lives in its adapter; the root of the build holds the one table of adapters |
| 114 | Before the first release the state database has one schema and no migrations: a state of another schema is refused (2026-11-17) |

## Harnesses — [adapters](../design/adapters.md), [contracts 4](../design/contracts.md#4-driver-interface)

| № | Decision |
|---|---|
| 3 | claude and codex in the first release, then opencode. One adapter per harness |
| 4 | Native protocols: claude is `claude -p` stream-json, codex is loop-manager's own `codex app-server`. The driver interface follows the semantics of ACP v2 |
| 9 | The instance registry is lazy: an instance starts on first need for a concrete set of settings |
| 38 | The codex adapter sets `sandbox_mode = "danger-full-access"` unless the loop-manager config or the host config sets it; `doctor` and the docs warn |
| 40 | Updating a harness is the user's concern, or the loop-manager developer's in this repository; it is no acceptance criterion |
| 45 | codex protocol types are read by hand, field by field, and held by contract tests; nothing is generated from the schema |
| 46 | codex with host inheritance and skills gets its own `CODEX_HOME` that inherits the host and adds the skills; a `config_error` on that path is forbidden |
| 59 | codex harness hooks get the attempt environment through an `export` at the start of the hook command; the attempt token visible in its argv is accepted |
| 73 | Model short names are the adapter's knowledge: a short name expands to the latest model of its family, a full name is passed as is |
| 109 | Mapping a short name to the current full name is the adapter's job, so that a user can say "the latest sonnet-class model". The claude adapter leaves it to the claude CLI, and its model gate key carries the name as the user wrote it (2026-11-17) |
| 116 | A run is unattended, and its harness decides permissions itself: claude runs with `--permission-prompts none`, codex with `approval_policy = "never"` unless the config sets another policy; what the config does not allow, the harness denies. Nothing waits for an answer: there is no `requires_action`, no `human_timeout`, no answer by a human (2026-11-17) |
| 117 | The default is permissive: unless the config says otherwise the claude adapter runs in `bypassPermissions` and the codex adapter with `approval_policy = "never"`, beside decision 38. A user's own policy is native permission rules and the harness hooks `PermissionRequest` and `PreToolUse`; restrictive presets come later (2026-11-17) |
| 118 | An adapter classifies a failure by the harness's structured fields; text is a narrow fallback, and what nothing tells apart is `unknown` with the raw value kept (2026-11-17) |
| 119 | An MCP server of a session that fails to start fails the attempt (`tool_failure`), in claude and codex alike, whoever configured it: loop-manager, the rig or the host (2026-11-17) |
| 124 | A permission the harness denied is a run event of its own, `run.permission_denied`, from what the adapter reports (claude's `permission_denied` and `result.permission_denials`) (2026-11-17) |

## Config — [config](../design/config.md)

| № | Decision |
|---|---|
| 6 | The hierarchy is orchestrator → project → rig |
| 7 | Config is layered; the effective config is assembled before a harness starts |
| 8 | Host inheritance is an option, on by default |
| 25 | External dependencies are managed through mise |
| 71 | Launch settings are three layered scalars: `harness`, `model`, `effort`. The agent gives defaults, the pack and the formula change them, the user's explicit setting beats all. A formula also sets its agents' native settings (`native.<harness>`, permissions among them) (revised 2026-11-17) |
| 72 | The user names a harness (`claude`, `codex`), not a provider; there is no `provider` setting |
| 89 | A harness the build has an adapter for works without a `[harness.<name>]` section; the section only sets its defaults |
| 90 | There is no neutral `permission_mode`: it is a native setting of a harness and lives in `native.<harness>`. No common section has a key only one harness understands |
| 91 | All a user says of an agent lives in `[agents.<name>]`: the name of a pack's agent edits it, a new name with `base` is a variant. There is no `[overrides.*]` |
| 99 | The root of a project has a name (default `root`) and a path (default: the directory of `loop-manager.toml`), both settings. It may be the only repository of the project, or a plain directory without git |
| 120 | Everything a run needs is decided before its start, through the layers default → orchestrator config → project config → formula; inside a step no human decides anything. Between steps a formula may wait for an outcome from outside, with an `await` step (decision 126) (revised 2026-11-17) |
| 129 | `rounds` is layered: orchestrator config → project config → formula, default 3. A return from an `await` step spends a round like any other (2026-11-17) |
| 130 | An input of a formula takes its default from the orchestrator config and the project config; a value given at the start, by a trigger or by the step of a calling formula beats them (2026-11-17) |
| 137 | An `exec` step (and a check script, which gets its environment) starts with a clean environment by default. The orchestrator config, the project config (`[exec]`) and the step itself take two keys: `inherit_env` (a boolean, the daemon's whole environment) and `env` (a table, merged by key). An `env` value is a literal or names a variable of the daemon's environment as `${NAME}` (`$$` is a literal `$`); this goes for the `env` of agents too. A reference is resolved when the process starts and is never stored resolved: the run's snapshot, the journal and `config explain` show `${NAME}`. A reference to a variable the daemon does not have fails the step as `config_error`, naming the variable (2026-11-17) |

## Agents, packs, formulas — [config](../design/config.md)

| № | Decision |
|---|---|
| 13 | A formula is a name, a description, inputs and steps in order; a step is an agent's turn (`ai`), an exec command, a `drain` or another formula |
| 19 | A pack and an addon are two different things |
| 60 | A prompt is required: a run without one does not start; an empty role file or template is a config error |
| 68 | An agent is a general role with default launch settings; it knows nothing of a concrete task |
| 69 | A formula answers "how"; the "what" comes only from the user's input |
| 70 | A pack holds agents and formulas fit for any project, with not a word about a concrete task |
| 75 | An agent variant is a pack's agent under the user's own name with other settings; a rig maps an agent name to a variant |
| 80 | A formula is a workflow: a sequence of agent calls and checks between them, by a script or an agent |
| 83 | A `drain` step gives out every bead under the root bead and waits for them; how many at once and what to do on a failure are settings of the step; by default one failed bead does not stop the others |
| 84 | A check step may send the formula back to an earlier step with its remarks; the formula bounds the number of rounds |
| 85 | A pack comes from a directory or a git repository pinned by sha or tag, the latest revision without a pin; `loop-manager` downloads it; tags are immutable; a CLI command clears the cache |
| 93 | A step has optional `max_attempts` (default 1) and `step_check_script_path` (default: none); the script decides whether the step passed: 0 passed, 75 a failure with no repeat, anything else not passed |
| 110 | `max_attempts` of a step is at most 20, the ceiling of a run's session attempts (2026-11-17) |
| 111 | Exit 75 of a step's check script is a fatal failure with nothing to repair: the step is not repeated and a check step with `back_to` sends nothing back; the run fails at once (2026-11-17) |
| 94 | An agent's turn is `kind = "ai"`; there is no step kind `prompt` |
| 95 | An agent's `scope`: `rig` is bound to a rig and works in its directory; `root` is bound to no repository and works over all rigs and the shared bead space. Planning over several repositories is `root` only |
| 97 | An agent's prompt template lies in its directory as `prompt.template.md`: an agent is self-contained. A project replaces it with `.loop-manager/agents/<agent>/prompt.template.md` in its root, without forking the pack |
| 98 | Text a project gives an agent (check conditions and the like) is a file in `.loop-manager/agents/<agent>/` of the root, not a string in `loop-manager.toml` |
| 100 | `drain`: `pool` is how many beads one step keeps in work at once; default 1 |
| 101 | `drain`: `on_item_failure` is `continue` (default), `soft_stop` or `hard_stop` |
| 102 | A step may be another formula; the parent's inputs are inherited, but only those the child declares; a step may set them explicitly |
| 103 | The daemon is system-wide, formulas live in the scope of a project: a project sees only the formulas of its packs. The structure is flat. `internal = true` hides a formula from the catalogue and allows it only as a part of another (`--all` shows it, the API marks it) |
| 126 | A formula step of kind `await` waits for an outcome from outside: it has no process and holds no concurrency slot, and its outcome (`pass` or `fail`, a summary, data) is given through the API and the CLI by a human, a script or an addon. It is a check like any other: `fail` sends the run back to its `back_to`, and the outcome names no other step. A run does not reach it while a check before it has failed. Entering it is an event, `run.awaiting`. Decision 116 stands: no live session waits for anybody (2026-11-17) |
| 127 | An `await` step ends by its `timeout`, 7 days by default: with `fail_on_timeout = true`, the default, the run fails with no return to `back_to`; with `false` the step passes (2026-11-17) |
| 128 | A step may carry `if`: a `where` table (contracts 2.2) over the run context of a prompt template and `back_from`, the names of the checks that sent the run back into the current round. A step whose `if` does not match is skipped. There is no expression language, and `back_to` stays the only jump (2026-11-17) |
| 134 | Review feedback is worked by an agent of its own on the formula, with its own skills, instructions and permissions, not by the implementer (2026-11-17) |
| 135 | The working formula of a feature over several repositories has its gate on the epic: an internal `-base` formula plans and gives the beads out, and the formula around it opens the pull requests, waits for the root's to be merged and merges the children's. A variant with a gate per repository is not written (2026-11-17) |
| 148 | An agent sees the comments of its task in its prompt and, where the formula needs them, the comments and the fields of the task's parents: the whole picture and the place of the task in it. Data that goes into an agent's context is given as TOON, not JSON (fewer tokens); wherever else data reaches a context, TOON is preferred where that is reasonable (2026-11-19) |

## Runs — [contracts 6](../design/contracts.md#6-run), [invariants](../design/invariants.md)

| № | Decision |
|---|---|
| 15 | A run starts by an explicit call, a subscription to an event or a schedule; all three are in the core |
| 16 | The outcome is the agent's explicit finisher; a turn that ends without one is a failure |
| 17 | A new session per run, per `ai` step in a formula of several; another run's session is continued only explicitly |
| 47 | An exec step gets its context through the environment (`LOOP_MANAGER_RUN`, `LOOP_MANAGER_BEAD`, `LOOP_MANAGER_INPUT_<NAME>`); nothing is substituted into the command; input types are checked at start |
| 50 | Waiting on the gate of the run's own account after `auth_required`, `billing` or `quota_exhausted` does not spend the attempt ceiling; any other parking does |
| 51 | Schedules use the system time zone; the orchestrator's `timezone` key overrides it |
| 58 | A run that waited for a decision on a deciding event waits for the same decision after a restart; a declared outcome and the result of an exec command are not lost |
| 74 | The model gate key is `model:<harness>/<expanded name>`; a run has one model, and the key is known from the config before the start |
| 76 | A run started by a `drain` step, a `formula` step or from an agent's session inherits the parent's correlation, one level deeper, under the common depth limit |
| 77 | `loop-manager run wait` waits for the terminal state of runs by events, not by polling |
| 104 | Once the outcome is declared and the turn has ended, background processes the agent left do not hold the run: the daemon stops them |
| 108 | The core does not hold a workdir: a run is bound to a directory and loop-manager does not lock it. Several runs may work in one rig's directory at once; keeping them apart is the user's choice: `worktree = true`, or the rig's `max_writers`, by which the core queues the runs that work in the rig's own directory (no limit by default) (2026-11-17, revised 2026-11-19) |
| 121 | The watchdog's defaults are `silence` 15 min, `tool_silence` 40 min, `background_silence` 120 min. Its keys are cut-offs only: observing and monitoring runs are addons (2026-11-17) |
| 122 | Inside a live run the default policy recovers by itself (`resume`, `new_session`). A run that failed for good is recovered by hand: no recovery scenario is predictable, so loop-manager gives tools, not a scenario. A user sees which runs and sessions failed and with what failure kinds, and decides: continue a session, a new session on the same task with or without a note, or abandon it. A skill helps the user's agent sort out the aftermath. Formulas that recover by themselves are a low-priority [roadmap](../roadmap.md) item (revised 2026-11-17) |
| 146 | An agent's turn ends its session. Each entry into an `ai` step is a new session: a return to the step for a new round and a repeat under `max_attempts` start one, with the step's prompt and the remarks, not a continuation of the old context. The recovery of a failed attempt by `resume` (decision 122), after a daemon crash among others, still continues the session of that entry, and `continue_from` still continues another run's session explicitly (2026-11-19) |

## Tracker — [tracker](../design/tracker-br.md)

| № | Decision |
|---|---|
| 11 | br behind an adapter interface; one database per project; the rig of a bead is the prefix of its id |
| 18 | A task is an optional input; the daemon takes up or files the bead for it, and claims, returns and closes it. `pass` closes the bead; the next work comes by taking the next ready bead, within an epic too |
| 32 | br is installed apart, through mise, and declared there; its license gets an explicit warning wherever a user meets br |
| 52 | A tracker operation is added to the interface only when a caller needs it; a bead of another rig is refused |
| 81 | The user's request becomes the root bead; everything filed for it lies under that bead |
| 82 | An executor's bead carries all it needs: the task, the rig, the agent and the harness. Which beads to file and for whom is the planner's judgement |
| 92 | The user gives a formula a task, not a bead: nobody files one in advance. A task that names an existing bead takes it up; otherwise `loop-manager` files the root bead |
| 96 | A rig's bead carries the rig's prefix: the acronym of its name (`backend` → `b`, `my-project` → `mp`) or the rig's `prefix` key; the root's beads carry no rig prefix |
| 147 | A failure of a bead is one that cannot be repaired: the rounds or the attempts for the work ran out. Red tests on a second round, a reviewer sending the work back, one more round are not failures. The key that bounds the failures of a bead says so by its name and by what it counts: `bead_failed_runs`, the runs of the bead that ended `failed` (2026-11-19) |

## Addons — [addon guide](../guide/addons.md), [writing an addon](../guide/addon-development.md), [contracts 3](../design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts)

| № | Decision |
|---|---|
| 20 | An addon's logic on the orchestrator side is exec hooks on core events and supervised services |
| 21 | A worktree is an optional part of a run |
| 24 | A shared MCP server is one per scope, shared by sessions, not started per session |
| 41 | The stdio→HTTP bridge is in the first release |
| 42 | A service is any long-lived supervised process; MCP is optional for it |
| 48 | A worktree is switched on by the formula setting `worktree = true`; there are no central lists and no agent-level switch |
| 49 | `auth_required` and `billing`: repeats with a growing pause up to once an hour, never escalated to a human |
| 123 | Compaction, metrics, budgets and a notice that a harness is unusable are addons, not the core; they are on the [roadmap](../roadmap.md), which is a hint, not a work list. A loop without progress is no problem of the core (2026-11-17) |
| 125 | loop-manager's skills for a user's agent ship as a plugin of the harness's own plugin system, for Claude Code and Codex only (2026-11-17) |
| 131 | The first bridge from an outside reviewer to an `await` step is GitHub: a process outside the core that polls GitHub, which does not push to a local machine. A merged pull request is `pass`, and nothing else is an acceptance: whoever merged decided. A pull request closed without a merge cancels the run. A comment or a review by an allowed author that no agent has picked up yet is `fail` (2026-11-17) |
| 132 | Whose comments reach an agent is the formula input `feedback_from`, a list of logins, empty by default: then nobody's do, the author's among them. The comments of anyone else never reach the agent (2026-11-17) |
| 133 | What a cancelled run left behind is cleaned by a formula that a trigger starts on `run.cancelled`, and on nothing else: a failed run is looked into by a human, not cleaned (2026-11-17) |
| 141 | An addon is loop-manager's own format (importing the native plugins of Claude Code or Codex is a later question). An `[[addon]]` declaration only names its source: a `path`, or `git` with `rev` and an optional `dir`. Attaching it is the one switch: `addons = [{ name, params }]` in `[settings]` of the orchestrator or a project gives it to every agent, on an agent, a variant or an edit to that agent only; an agent has 0..N addons, applied in the order of their first mention. An addon's `[params]` carry defaults (the default gives the type); any layer of an attachment overrides them by key, and an unknown key or a wrong type is a load error. Params reach the agent side as `${params.<key>}` in its strings and the orchestrator hooks as `LOOP_MANAGER_ADDON_PARAMS`, the values in force for the run's agent; an orchestrator hook runs for the runs whose agent attaches the addon. `harnesses` in `addon.toml` may restrict an addon to some harnesses. A marketplace and installing from the CLI come later: the config is edited by hand (2026-11-18) |
| 142 | loop-manager's own addons live in `addons/<name>/` of this repository, each self-contained, so moving one to a repository of its own changes only its declaration. `examples/` is gone. The plugin of the operator's harness that drives loop-manager (decision 125) is a different thing and lives in `plugin/` (2026-11-18) |
| 143 | An agent may call native subagents of its harness: `subagents` in the agent lists the agents (and variants) it may call, by name or with `*` (`qa-*`); by default only itself. The list is per task: the session gets the definitions of its agent's list, and a nested subagent calls within the same set. A callable agent needs a `description`. The role prompt is appended to the harness's system prompt and never replaces it; the prompt template stays the user prompt of a run; a subagent gets its role prompt as its native prompt. A native subagent is not a run: no finisher, gates, `max_runs` or orchestrator hooks. loop-manager does not limit nesting: the harness's own limits apply (claude: depth 2, `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`; codex: its thread limit), and codex's built-in agent types stay visible; the docs say so and leave both to the user. The frontends show the call graph (2026-11-18) |
| 144 | The first addon of loop-manager is `context-budget`, for claude and codex: it sets the context window at which the harness compacts a session, counts the compactions, keeps the label `compactions:<n>` on the run's bead, and ends the run past `max_compactions`. A demo of its own shows a small budget, the addon's config and its work (2026-11-18) |
| 145 | loop-manager's unit of extension is an **addon**; the word plugin means only a native plugin of a harness (Claude Code, Codex). The names follow: `addon.toml`, `[[addon]]`, `addons = [{ name, params, enabled }]`, `loop-manager addon list`, `LOOP_MANAGER_ADDON_*`, the events `addon.*`, `addons/<name>/` of this repository, the bundled addons and their crate `loop-manager-addons`. loop-manager's skills plugin for the operator's harness (decision 125) moves from `operator-plugin/` to `plugin/`. Every decision and assumption that says plugin of loop-manager's own unit reads addon, its sense unchanged; the rename edits their wording. Nothing of the old names is kept (decision 140) (2026-11-18) |

## Frontends — [frontends](../api/frontends.md), [MCP](../api/mcp.md)

| № | Decision |
|---|---|
| 10 | HTTP API, CLI and MCP are three adapters over one core. The API and the CLI are the full surface, MCP a small one |
| 78 | The MCP tool set does not grow: a planner agent uses the CLI |
| 112 | A request that names a thing that does not exist answers 404 `not_found` on every method; 409 `rejected` is for a thing that exists and may not do this now (2026-11-17) |
| 113 | The daemon is the backend and the CLI its frontend: `config check`, `config explain` and `doctor` ask the daemon like every other command. Only with an explicit `--config <file>` do they work on that file without a daemon (2026-11-17) |

## Delivery — [install](../guide/install.md), [development](../guide/development.md)

| № | Decision |
|---|---|
| 29 | Checks run on the owner's host only (Linux arm64); the other three targets are built |
| 63 | The four targets are built only for a release (`mise run acceptance-release`, the workflow `release` on a `v*` tag): development, the gate and a local acceptance run build the host's target only. No tag is made only to check the build |
| 66 | `br` is pinned by an exact version, claude and codex by a release line; there is no `mise.lock` |
| 87 | mise is a convenience, not a requirement, for development and for the demos; a demo's dependencies are in its own `mise.toml` without a lock |
| 138 | loop-manager follows the current versions of the harnesses and tools it works with: a legacy or compatibility format or method (a manifest layout, a flag, an API a tool marks as legacy) is used only on the owner's explicit request (2026-11-17) |

## Process — [code rules](rules-code.md), [test rules](rules-tests.md)

| № | Decision |
|---|---|
| 14 | Acceptance scenarios with real runs on a cheap model run only on the owner's explicit request |
| 23 | Line budgets are checked by CI |
| 26 | Every measurement carries a visible date, a version or short sha, and an invisible `MEASURE` comment |
| 27 | No stubs and no code "for later"; less code is better |
| 30 | There are no deadlines and no calendar budgets |
| 35 | CI has no access to an LLM, by design. Whatever needs a login and real tokens runs only on a developer's machine, by an explicit command, on the cheapest model and narrowly |
| 36 | Burning quota on purpose for the sake of a check is forbidden |
| 37 | Fast tests come before new features: a slow or flaky suite is fixed before anything new is built |
| 39 | The demos are `demos/1-planner-executor/` and `demos/2-multirepo-qa/` |
| 44 | An acceptance run writes one report with every criterion, the rows that live in cargo tests and scripts among them |
| 54 | Results of experiments go to `docs/design/experiments.md` and the code of a recorded experiment is deleted; experiment code stays under 5 000 lines; each of `experiments/*`, `demos/*`, `examples/*` has a short README |
| 56 | No locked seeds: a test passes on any seed; a random stand takes a new seed each run and prints it |
| 57 | Unit and integration tests are the everyday loop; nothing new is built on a slow or unstable stand |
| 64 | The repository is public: research reports are not in it, and no other project of the owner, the predecessor among them, is named in it |
| 67 | Temporary logs of the work process are not committed; git history is not rewritten |
| 79 | The averaging window of the idle-CPU check is 30 seconds |
| 86 | Running a demo does not conflict with the files of the repository: what a run creates is in `.gitignore`; the author's result lies beside it with the suffix `-example` and the date it was made |
| 88 | The core's budget is 18 000 lines |
| 115 | The integration tests of the core and those of the binary are one test binary each (`crates/loop-manager-testkit/tests/core`, `crates/loop-manager/tests/e2e`): a new test is a module there, not one more binary (2026-11-17) |
| 105 | `mise run ci` is the gate of every change. Acceptance scenarios, live models and the demos run only on the owner's explicit request, and are a criterion of done only when the owner says so (2026-11-17) |
| 106 | There is no spec and no readiness file: `docs/` describes the product as it is, `docs/agents/` holds the rules for agents (2026-11-17) |
| 107 | The chaos stand, with all its 1000 seeds, is an acceptance row, not a part of the gate. Acceptance may be slow: its model-free rows run only in CI, in a workflow started by hand, on the owner's request (2026-11-17) |
| 136 | Demo 3 shows the `await` step on GitHub and is part of the acceptance of that work: public MIT repositories of the GitHub user `kvokka-agent`, a root and its children, with no `loop-manager` in their names; the models are `luna` and `terra`; an agent plays the reviewer and merges. Agents may create these repositories, recreate them and open new pull requests there (2026-11-17) |
| 139 | The live run of demo 3 is part of acceptance criterion 20 and of `mise run acceptance`, next to demos 1 and 2 (2026-11-17) |
| 140 | No users and no first release: nothing is kept for compatibility. A design that turns out to be a placeholder (the addon model first) is replaced whole, its code and docs cut in the same change, with no transition layer (2026-11-18) |

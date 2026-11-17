# Glossary

Every term the Markdown files under `docs/` define, with a one-line definition and the document that defines it.
Terms are listed by the document where they are first defined; a term used across documents is listed once.

## Product and overview

| Term | Definition | Defined in |
|---|---|---|
| loop-manager (Adaptive Agent Executor) | An event-driven orchestrator for coding agents: one daemon per user, running formulas on agents through their harnesses | [overview](docs/overview.md) |
| orchestrator | The `loop-manager` daemon and its machine-level settings | [overview](docs/overview.md#glossary) |
| project | A directory with `loop-manager.toml`: a root, a set of rigs, packs, addons and one tracker database | [overview](docs/overview.md#glossary) |
| root (of a project) | The project's root directory, where agents of scope `root` work; named `root` and located at the `loop-manager.toml` directory by default, both settings; need not be git | [overview](docs/overview.md#glossary) |
| rig | A repository of a project on disk, with its bead prefix and variables such as `test_cmd` and `lint_cmd` | [overview](docs/overview.md#glossary) |
| harness | A kind of agent CLI (`claude`, `codex`), with one adapter each | [overview](docs/overview.md#glossary) |
| harness instance | A running harness process (an app-server or a session process) serving one or more sessions | [overview](docs/overview.md#glossary) |
| agent | A general role (a role prompt, prompt template, skills, scope and default launch settings) that knows nothing of a concrete task | [overview](docs/overview.md#glossary) |
| agent scope | `rig`: the agent works in one rig; `root`: it works in the root, over all rigs of the project | [overview](docs/overview.md#glossary) |
| launch settings | The three scalars harness, model and effort that decide how a session is started | [overview](docs/overview.md#glossary) |
| model short name | A family name (`opus`, `luna`) that the adapter expands to the latest model of that family | [overview](docs/overview.md#glossary) |
| agent variant | A pack's agent under another name with other settings, declared in the user's config | [overview](docs/overview.md#glossary) |
| subagent | An agent a session may call as a native subagent of its harness; not a run | [overview](docs/overview.md#glossary) |
| session reference | The pair "harness + the harness's session id": all the core knows of a conversation | [overview](docs/overview.md#glossary) |
| formula | A workflow (the "how"): a name, a description, inputs and steps in order | [overview](docs/overview.md#glossary) |
| step | One item of a formula: `ai`, `exec`, `drain`, `formula` or `await` | [overview](docs/overview.md#glossary) |
| task | The text a user gives a formula at start: what to do; it names an existing bead or becomes a new root bead | [overview](docs/overview.md#glossary) |
| run | One execution of a formula with concrete inputs; it has attempts and an outcome | [overview](docs/overview.md#glossary) |
| child run | A run started by a `drain` or `formula` step of another run, or from an agent's session; it inherits the correlation | [overview](docs/overview.md#glossary) |
| attempt | One start or resume of a session within a run | [overview](docs/overview.md#glossary) |
| finisher | The call by which an agent reports the outcome of its step (`loop-manager finish`) | [overview](docs/overview.md#glossary) |
| trigger | A rule "an event or a schedule starts a formula" | [overview](docs/overview.md#glossary) |
| correlation | The id of a chain of runs that started one another | [overview](docs/overview.md#glossary) |
| pack | Content fit for any project: agents, formulas and triggers | [overview](docs/overview.md#glossary) |
| addon | An extension, a directory with `addon.toml`, attached per agent or for all, with parameters | [overview](docs/overview.md#glossary) |
| service | A long-lived process the orchestrator supervises, such as a shared MCP server | [overview](docs/overview.md#glossary) |
| issue gate | A record that starts no run by a key until a time or until it is opened by hand | [overview](docs/overview.md#glossary) |
| tracker | The task store behind an adapter interface; the first implementation is br | [overview](docs/overview.md#glossary) |
| bead | A task in the tracker: real work only, no bookkeeping records | [overview](docs/overview.md#glossary) |
| root bead | The bead holding the user's task; every bead filed for that task lies under it | [overview](docs/overview.md#glossary) |
| event | A record of what happened: a reason to re-read the state, not the truth | [overview](docs/overview.md#glossary) |

## Architecture and event model

| Term | Definition | Defined in |
|---|---|---|
| core | The minimal part that holds what a run needs to start, be observed and end correctly | [architecture](docs/architecture.md), [overview](docs/overview.md) |
| adapter | A component that binds the core to an external system: a harness, a tracker or a frontend | [architecture](docs/architecture.md) |
| frontend | An adapter exposing the core to people and tools: the HTTP API, the CLI and MCP | [architecture](docs/architecture.md), [frontends](docs/api/frontends.md) |
| single writer | The core's rule that one writer changes state, one SQLite transaction per message, with effects run after commit | [event-model](docs/design/event-model.md) |
| `loop-manager-core` `Msg` | The message type of the single writer in `loop_manager_core::run` | [event-model](docs/design/event-model.md) |
| deadline | A time at which the core must react, set by `Tx::set_deadline` and cleared by `Tx::clear_deadline`; a safety timer, not a poll | [event-model](docs/design/event-model.md) |
| effect | Work the core asks for after a commit (a process start, a call), retried with backoff (500 ms, 1 s, 2 s, capped at 5 min) | [event-model](docs/design/event-model.md) |
| state database | The SQLite store in WAL mode at `$XDG_STATE_HOME/loop-manager/state.db`, with `user_version` as its schema version | [event-model](docs/design/event-model.md) |
| journal | The JSONL event log at `$XDG_STATE_HOME/loop-manager/journal/events.jsonl`, with one `seq` counter per orchestrator, rotated at 10 MB, 5 files kept | [event-model](docs/design/event-model.md), [overview](docs/overview.md) |
| seq | The journal's sequence number, by which a reader continues; a rotated-away `seq` gives an explicit gap | [event-model](docs/design/event-model.md), [acceptance](docs/acceptance.md) |
| resync | The periodic re-read of state (every 5 minutes, `daemon:resync`); a diverging result is `resync.diverged` | [event-model](docs/design/event-model.md) |
| invariant | One of the 20 rules the core keeps after every step | [invariants](docs/design/invariants.md) |
| recovery step | A step the core takes to restore a run after a restart or a failure | [invariants](docs/design/invariants.md) |
| prompt delivery | "At least once, with an idempotent effect" | [invariants](docs/design/invariants.md) |
| drain (of the daemon) | A planned restart: `POST /v1/daemon/drain`, with a 300 s drain timeout | [invariants](docs/design/invariants.md) |
| chaos stand | The randomized check of the core over 1000 seeds, with `LOOP_MANAGER_CHAOS_SEED` and `LOOP_MANAGER_CHAOS_SEEDS` | [development](docs/guide/development.md#the-chaos-stand), [acceptance](docs/acceptance.md) |
| crate | A Rust crate of the workspace: `loop-manager-contract`, `loop-manager-core`, `loop-manager-claude`, `loop-manager-codex`, `loop-manager-br`, `loop-manager-front`, `loop-manager-addons`, `loop-manager`, `loop-manager-testkit`, `loop-manager-acceptance` | [architecture](docs/architecture.md) |
| operator plugin | The plugin of the operator's harness that drives loop-manager, in `plugin/` | [decisions](docs/agents/decisions.md) (142) |

## Contracts

| Term | Definition | Defined in |
|---|---|---|
| event envelope | The shape every event is written in | [contracts §1.1](docs/design/contracts.md#1-event) |
| `RunView` | The read model of a run as a frontend and a hook see it | [contracts §1](docs/design/contracts.md) |
| `Failure` | A typed failure of an attempt: a kind and its fields | [contracts §4.4](docs/design/contracts.md#4-driver-interface) |
| `Bead` | The bead as the core sees it (task, rig, agent, harness) | [contracts §1](docs/design/contracts.md) |
| `Gate` | The issue gate as the core sees it | [contracts §7](docs/design/contracts.md#7-issue-gates) |
| `Attempts` | The count of a run's session attempts against its ceiling | [contracts §6](docs/design/contracts.md#6-run) |
| `Outcome` | The declared result of a step: `pass` or `fail` with a summary and data | [contracts §6](docs/design/contracts.md#6-run) |
| `where` matcher | A filter over event fields, the filter language of triggers and hooks | [contracts §2](docs/design/contracts.md#2-filter-and-template-language) |
| template language | The `{{ … }}` substitution in prompt templates | [contracts §2](docs/design/contracts.md#2-filter-and-template-language) |
| hook | An exec command run on a core event; its decision is exit 0, 2 (block) or other (failure) | [contracts §3](docs/design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts) |
| decision event | The event recording a hook's decision | [contracts §3](docs/design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts) |
| outbox lifetime | How long an outbound message of the core is kept | [contracts §3.5](docs/design/contracts.md#3-orchestrator-hooks-decisions-and-timeouts) |
| `Driver` | The harness interface (§4), with an `ADAPTER` const per harness | [contracts §4](docs/design/contracts.md#4-driver-interface), [adapters](docs/design/adapters.md) |
| failure kind | The class of a harness failure, e.g. `rate_limited`, `quota_exhausted`, `billing`, `auth_required`, `overloaded`, `context_overflow`, `config_error`, `harness_crash`, `tool_failure`, `unknown` | [contracts §4.4](docs/design/contracts.md#4-driver-interface) |
| capabilities | What a harness adapter declares it can do (its tested `versions` among them) | [contracts §4.5](docs/design/contracts.md#4-driver-interface) |
| `Tracker` | The tracker interface (§5) and its `TrackerError` | [contracts §5](docs/design/contracts.md#5-tracker-interface) |
| run states | The states of a run (§6.1–6.2), including `passed`, `failed` and `waiting_for`; a run classes its failure | [contracts §6](docs/design/contracts.md#6-run) |
| finisher (contract) | The `loop-manager finish pass\|fail` contract (§6.3); a turn that ends without it is a failure | [contracts §6.3](docs/design/contracts.md#6-run) |
| prompt acked | The `PromptAcked` event: the harness took the prompt | [adapters](docs/design/adapters.md) |
| turn ended | The `TurnEnded` event: a harness turn finished | [adapters](docs/design/adapters.md) |
| `stop_background` | The adapter operation that stops background work a session left | [adapters](docs/design/adapters.md) |
| `SessionRef` | The adapter's reference to a session (see session reference) | [adapters](docs/design/adapters.md) |
| issue gate key | A gate key of the form `bead:<project>/<id>`, `rig:<project>/<rig>`, `account:` or `model:<harness>/<name>` | [contracts §7](docs/design/contracts.md#7-issue-gates), [decisions](docs/agents/decisions.md) (74) |
| instance registry | The registry of harness instances, keyed by fingerprint; lazy by decision 9 | [contracts §8](docs/design/contracts.md#8-instance-and-service-registry) |
| service registry | The registry of supervised services | [contracts §8](docs/design/contracts.md#8-instance-and-service-registry) |
| fingerprint | The key of an instance: the concrete set of settings it was started with | [contracts §8](docs/design/contracts.md#8-instance-and-service-registry) |
| `LOOP_MANAGER_INSTANCE` | The environment variable naming the instance a process belongs to | [contracts §8](docs/design/contracts.md#8-instance-and-service-registry) |
| default policy | The orchestrator's built-in decisions for a run (retry, resume, new session), in `default-policy` | [contracts §10](docs/design/contracts.md#10-default-policy-as-decisions) |
| attempt token | The token that identifies an attempt, so that a stale attempt's finisher is refused | [contracts §6.3](docs/design/contracts.md#6-run) |

## Adapters

| Term | Definition | Defined in |
|---|---|---|
| `ADAPTER` const | The constant every harness adapter exports: `name`, `models`, `tested`, `agent_types`, `subagents_inherit`, `new`, `doctor` | [adapters](docs/design/adapters.md) |
| `loop_manager_contract::driver::child` | The shared helper of the harness child processes | [adapters](docs/design/adapters.md) |
| `${LOOP_MANAGER_SERVICE_PORT:<id>}` | The port reference of a service, resolved at start | [adapters](docs/design/adapters.md) |
| contract suite | The 15 scenarios each harness adapter must pass against fake provider APIs, plus codex's 16th | [adapters](docs/design/adapters.md), [harness-updates](docs/harness-updates.md) |
| `loop-manager-claude` | The claude adapter: one `claude -p` per session, stream-json | [adapters](docs/design/adapters.md), [architecture](docs/architecture.md) |
| `loop-manager-codex` | The codex adapter: one `codex app-server` per fingerprint over stdio | [adapters](docs/design/adapters.md), [architecture](docs/architecture.md) |
| ACP v2 semantics | The model the driver interface follows, without ACP as transport | [decisions](docs/agents/decisions.md) (4), [overview](docs/overview.md) |
| `permission_mode` | A claude-native setting; lives in `native.claude` | [decisions](docs/agents/decisions.md) (90), [config](docs/design/config.md) |
| `approval_policy` | The codex-native setting that decides approvals; `never` by default | [decisions](docs/agents/decisions.md) (38, 116, 117) |
| `sandbox_mode` | The codex-native sandbox setting; the adapter sets `danger-full-access` by default | [decisions](docs/agents/decisions.md) (38), [first-formula](docs/guide/first-formula.md) |
| `CODEX_HOME` | The home directory of a codex session, inheriting the host and adding skills | [decisions](docs/agents/decisions.md) (46) |
| `run.permission_denied` | The run event of a permission the harness denied | [decisions](docs/agents/decisions.md) (124) |

## Tracker (br)

| Term | Definition | Defined in |
|---|---|---|
| br (beads_rust) | The external tracker binary (0.7.6, pinned), behind the tracker adapter; one database per project | [tracker-br](docs/design/tracker-br.md) |
| `BEADS_DIR` | The environment variable pointing br at the project's database | [tracker-br](docs/design/tracker-br.md) |
| `BD_ACTOR` | The actor of a br operation, `loop-manager:<run>:<n>`, marking who holds a claim | [tracker-br](docs/design/tracker-br.md) |
| `BD_ISSUE_PREFIX` | The bead id prefix of a project | [tracker-br](docs/design/tracker-br.md) |
| `external_ref` | The bead field keyed `loop-manager-` plus 16 hex digits, linking a bead to its run | [tracker-br](docs/design/tracker-br.md) |
| claim | The hold a run takes on its bead for the run's whole life | [tracker-br](docs/design/tracker-br.md) |
| `next_ready` | The tracker input that gives the next ready bead; `nothing_ready` when none | [tracker-br](docs/design/tracker-br.md) |
| `bead_failures` | The number of failures after which a bead is no longer given out (default 3) | [tracker-br](docs/design/tracker-br.md) |
| `under` | The br query for beads under a root (`br ready --unassigned --limit 0 --parent <root> --recursive`) | [tracker-br](docs/design/tracker-br.md) |
| `agent:` and `harness:` labels | The bead labels naming the agent and the harness that take the bead | [tracker-br](docs/design/tracker-br.md) |
| rig of a bead | The rig a bead belongs to: the prefix of its id | [tracker-br](docs/design/tracker-br.md), [decisions](docs/agents/decisions.md) (11, 96) |
| `compactions:<n>` label | The label of the `context-budget` addon that counts a run's compactions on its bead | [decisions](docs/agents/decisions.md) (144), [acceptance](docs/acceptance.md) |

## Config

| Term | Definition | Defined in |
|---|---|---|
| settings layers | The layers of config (default, orchestrator, project, rig, formula, agent) assembled before a harness starts | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (7) |
| "explain config" | The command showing the layer of every value | [config](docs/design/config.md), [acceptance](docs/acceptance.md) |
| `[[service]]` | The config table declaring a service | [config](docs/design/config.md) |
| `[[addon]]` | The config table declaring an addon's source (`path`, or `git` with `rev` and `dir`) | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (141) |
| `[inputs]` | The table of a formula's inputs | [config](docs/design/config.md) |
| `[exec]` | The table of exec environment settings: `inherit_env`, `env` | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (137) |
| `[root]` | The table giving the root's name and path | [config](docs/design/config.md) |
| `[rigs.<name>]` | The table declaring a rig of a project | [config](docs/design/config.md) |
| `[[trigger]]` | The table declaring a trigger (`on`, `cron`, `every`, `where`) | [config](docs/design/config.md) |
| `[agents.<name>]` | The table editing a pack's agent or declaring a variant | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (91) |
| `native.<harness>` | The table of a harness's native settings (permissions among them) | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (71) |
| `inherit_host` | The option taking the host's harness configuration into a session; on by default | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (8) |
| `${NAME}` | An environment reference in config values, resolved when the process starts; `$$` is a literal `$`, any other `$` is a load error | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (137) |
| `LOOP_MANAGER_INPUT_<NAME>` | The environment variable through which an exec step receives an input | [config](docs/design/config.md) |
| `LOOP_MANAGER_STEP_<NAME>_RESULT` | The environment variable carrying a named step's result | [config](docs/design/config.md) |
| `LOOP_MANAGER_REMARKS` | The environment variable carrying the remarks of a return | [config](docs/design/config.md) |
| `LOOP_MANAGER_RUN`, `LOOP_MANAGER_BEAD` | The environment variables naming the run and its bead for an exec step | [decisions](docs/agents/decisions.md) (47) |
| pack (`pack.toml`) | A directory or git repository of agents and formulas, pinned by sha or tag | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (70, 85) |
| addon (`addon.toml`) | The addon manifest: `harnesses`, `prompt`, `[params]`, `[agent]`, `[[hook]]`, `[[service]]` | [config](docs/design/config.md), [addon-development](docs/guide/addon-development.md) |
| addon params | The addon's `[params]`: defaults that any attachment layer overrides by key; `${params.<key>}` on the agent side, `LOOP_MANAGER_ADDON_PARAMS` for hooks | [addon-development](docs/guide/addon-development.md), [decisions](docs/agents/decisions.md) (141) |
| `${LOOP_MANAGER_ADDON_DIR}`, `${LOOP_MANAGER_ADDON_DATA}` | The addon's own directory and its data directory | [addon-development](docs/guide/addon-development.md) |
| `addons` (attachment) | The key `addons = [{ name, params, enabled }]` attaching an addon to agents, a variant or all agents | [addons](docs/guide/addons.md), [decisions](docs/agents/decisions.md) (141) |
| `default-policy` | The built-in addon of default decisions, with its parameters | [config](docs/design/config.md) |
| `worktree` | The addon, and the formula setting `worktree = true`, giving a run its own git worktree on branch `loop-manager/<run>` | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (21, 48) |
| `context-budget` | The first addon of loop-manager: sets the compaction window, counts compactions, ends a run past `max_compactions` | [addon-development](docs/guide/addon-development.md), [experiments](docs/design/experiments.md), [decisions](docs/agents/decisions.md) (144) |
| agent definition (`agent.toml`) | The file of an agent's launch settings, `subagents` and `description` | [first-formula](docs/guide/first-formula.md), [config](docs/design/config.md) |
| `prompt.md`, `prompt.template.md` | An agent's role prompt, and its prompt template, both in its directory | [first-formula](docs/guide/first-formula.md), [decisions](docs/agents/decisions.md) (97) |
| `.loop-manager/agents/<agent>/` | The project's directory of agent files (prompt template, check conditions) in the root | [decisions](docs/agents/decisions.md) (97, 98) |
| `subagents` | The agent key listing the agents (by name or `*`) it may call as native subagents | [addons](docs/guide/addons.md), [decisions](docs/agents/decisions.md) (143) |
| `description` | The agent key required for a callable agent | [addons](docs/guide/addons.md), [decisions](docs/agents/decisions.md) (143) |
| `called_by` | The field of `loop-manager agent list` naming the callers of an agent | [addons](docs/guide/addons.md) |
| variant | See agent variant | [overview](docs/overview.md#glossary) |
| `max_attempts` | The step key giving the repeat count of a step (default 1, at most 20) | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (93, 110) |
| `step_check_script_path` | The step key naming a script that decides whether the step passed: 0 passed, 75 fatal, else not passed | [first-formula](docs/guide/first-formula.md), [decisions](docs/agents/decisions.md) (93, 111) |
| `back_to` | The check key sending the formula back to an earlier step with its remarks | [first-formula](docs/guide/first-formula.md), [decisions](docs/agents/decisions.md) (84) |
| `rounds` | The number of rounds a formula may take (default 3), layered | [first-formula](docs/guide/first-formula.md), [decisions](docs/agents/decisions.md) (129) |
| `if` | The step key with a `where` table over the run context; a step whose `if` does not match is skipped | [decisions](docs/agents/decisions.md) (128) |
| `back_from` | The names of the checks that sent the run back into the current round, usable in `if` | [decisions](docs/agents/decisions.md) (128) |
| `drain` | The step giving every bead under the root to executors and waiting for them; keys `pool` and `on_item_failure` | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (83, 100, 101) |
| `pool` | The number of beads a drain step keeps in work at once (default 1) | [decisions](docs/agents/decisions.md) (100) |
| `on_item_failure` | The drain setting: `continue` (default), `soft_stop` or `hard_stop` | [decisions](docs/agents/decisions.md) (101) |
| `formula` step | A step running another formula as a child run; `internal = true` hides a formula from the catalogue | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (102, 103) |
| `internal` | The formula key hiding it from the catalogue; shown with `--all` | [decisions](docs/agents/decisions.md) (103) |
| `await` step | A step waiting for an outcome from outside; no process, no slot; ends by `timeout` (7 days default) with `fail_on_timeout` | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (126, 127) |
| `run.awaiting` | The event of entering an `await` step | [decisions](docs/agents/decisions.md) (126) |
| `feedback_from` | The formula input listing the logins whose review comments reach an agent | [decisions](docs/agents/decisions.md) (132) |
| `scope` | The agent key `rig` or `root` | [config](docs/design/config.md), [decisions](docs/agents/decisions.md) (95) |
| `run.cancelled` | The event a cleanup formula's trigger reacts to | [decisions](docs/agents/decisions.md) (133) |

## Runs, recovery and frontends

| Term | Definition | Defined in |
|---|---|---|
| `loop-manager finish pass\|fail` | The finisher command giving a step's outcome | [first-formula](docs/guide/first-formula.md), [frontends](docs/api/frontends.md) |
| `waiting_for` | The field of a run showing what it waits for | [first-formula](docs/guide/first-formula.md) |
| `{{ steps.<name>.result }}` | The template reference to a named step's result | [first-formula](docs/guide/first-formula.md) |
| `worktree = true` | The formula setting giving a run a worktree | [first-formula](docs/guide/first-formula.md), [decisions](docs/agents/decisions.md) (48) |
| recovery class | The class of a failed run: `agent`, `check`, `hollow_done` or `infrastructure` | [recovery](docs/guide/recovery.md) |
| `--continue-from` | The option continuing a failed run from its session | [recovery](docs/guide/recovery.md) |
| refix formula | The formula with a `note` input that retries a failed run with a remark | [recovery](docs/guide/recovery.md) |
| `recover` skill | The skill helping a user's agent sort out a failed run | [recovery](docs/guide/recovery.md), [decisions](docs/agents/decisions.md) (114) |
| `not_found` | The 404 error for a thing that does not exist | [decisions](docs/agents/decisions.md) (112) |
| `rejected` | The 409 error for a thing that exists and may not be done now | [decisions](docs/agents/decisions.md) (112) |
| `Idempotency-Key` | The request header making a retried API call safe; reuse with a different body gives 422 `idempotency_key_reused` | [frontends](docs/api/frontends.md) |
| `gap` | The 410 answer when a `seq` was rotated away | [frontends](docs/api/frontends.md) |
| `api_tcp` | The setting exposing the API over TCP with a bearer token | [frontends](docs/api/frontends.md) |
| MCP tools | The five tools: `list_formulas`, `start_run`, `run_state`, `list_runs`, `cancel_run` | [mcp](docs/api/mcp.md) |
| protocol epochs | The MCP protocol versions `2026-07-28` and the `initialize` versions (2025-11-25, 2025-06-18, 2025-03-26) | [mcp](docs/api/mcp.md) |
| exit codes | CLI exit codes, including 124 (timeout) and 75 (fail for good) | [frontends](docs/api/frontends.md) |
| `LOOP_MANAGER_CRASH_SEED` | The variable replaying the pause seed of the crash scenario | [acceptance](docs/acceptance.md) |
| criterion | One of the acceptance criteria 1–35 (there is no 21) | [acceptance](docs/acceptance.md) |
| live row | An acceptance row calling a real model | [acceptance](docs/acceptance.md) |
| model-free row | An acceptance row needing no login | [acceptance](docs/acceptance.md) |
| `MEASURE` marker | `<!-- MEASURE id= date= ver= env= src= note="..." -->`: a machine-readable measurement with date and version | [rules-code](docs/agents/rules-code.md), [harness-updates](docs/harness-updates.md) |
| `--permission-prompts none` | The claude flag under which the harness decides permissions itself | [decisions](docs/agents/decisions.md) (116) |
| `bypassPermissions` | The claude permission mode of the default policy | [decisions](docs/agents/decisions.md) (117) |
| watchdog | The core's stall detector: `silence` 15 min, `tool_silence` 40 min, `background_silence` 120 min | [decisions](docs/agents/decisions.md) (121) |
| `max_compactions` | The `context-budget` limit on compactions past which the run ends | [decisions](docs/agents/decisions.md) (144), [experiments](docs/design/experiments.md) |
| `autoCompactWindow` | The claude setting of the window at which compaction starts (floor 100 000) | [experiments](docs/design/experiments.md) |
| `model_auto_compact_token_limit` | The codex setting of the compaction token limit | [experiments](docs/design/experiments.md) |
| `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` | The claude variable overriding the auto-compact percentage | [experiments](docs/design/experiments.md) |
| `PreCompact`, `PostCompact` | The harness hooks around compaction | [experiments](docs/design/experiments.md) |
| `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` | The claude variable limiting subagent nesting (depth 2) | [decisions](docs/agents/decisions.md) (143) |
| `collaboration` namespace | The codex namespace for multi-agent v1/v2 | [experiments](docs/design/experiments.md) |
| experiment | One of experiments 1–10 measuring the real harnesses; results in `design/experiments.md` | [experiments](docs/design/experiments.md) |
| opencode | The next harness, after a spike (experiment 7) | [overview](docs/overview.md), [experiments](docs/design/experiments.md) |
| tested range | The harness versions the adapter has passed: claude 2.1.322–2.1.325, codex 0.167.2–0.168.0 | [harness-updates](docs/harness-updates.md), [install](docs/guide/install.md) |
| release line | The `mise.toml` version selector (`claude = "2.1"`, `codex = "0"`) | [harness-updates](docs/harness-updates.md) |

## Development, rules and loop

| Term | Definition | Defined in |
|---|---|---|
| gate (`mise run ci`) | The single check every change passes before it is handed over | [development](docs/guide/development.md), [rules-tests](docs/agents/rules-tests.md), [decisions](docs/agents/decisions.md) (105) |
| budget (line) | The line limits: core 18 000; whole release 30 000; a file at most 400 lines; hard stop 35 000 | [rules-code](docs/agents/rules-code.md), [decisions](docs/agents/decisions.md) (88) |
| `mise` | The tool manager for external dependencies and tasks; a convenience, not a requirement | [install](docs/guide/install.md), [decisions](docs/agents/decisions.md) (25, 87) |
| `acceptance-ci`, `acceptance`, `acceptance-release` | The mise tasks running model-free rows, all rows, and the release build | [development](docs/guide/development.md), [acceptance](docs/acceptance.md) |
| demos | The live examples: `demos/1-planner-executor/` to `demos/4-…`, each with its own `mise.toml` | [acceptance](docs/acceptance.md), [decisions](docs/agents/decisions.md) (39, 86) |
| `-example` suffix | The suffix of an author's demo result beside the run's output | [decisions](docs/agents/decisions.md) (86) |
| loop | The autonomous cycle of agent roles that develops loop-manager itself | [loop](docs/agents/loop.md) |
| loop role | `overseer`, `coder`, `reviewer`, `expert`, `helper` or `subcoder` | [loop](docs/agents/loop.md) |
| `loopctl` | The script driving the loop's coder → reviewer cycles | [loop](docs/agents/loop.md) |
| `.loop/JOURNAL.md` | The loop's journal, in Russian; entry kinds ВОПРОС, НАХОДКА, РЕШЕНИЕ, БЛОКЕР | [loop](docs/agents/loop.md), [rules-code](docs/agents/rules-code.md) |
| context budget (session) | The soft limit 300k tokens (a hook asks to wind down) and the hard limit 420k (forced stop, a dump in `.loop/handoffs/`) | [loop](docs/agents/loop.md) |
| assumption | A choice that is not the owner's, recorded in `assumptions.md` and changeable with a journal entry | [assumptions](docs/agents/assumptions.md), [decisions](docs/agents/decisions.md) |
| decision | A decided owner choice, numbered and stable; not revisited without the owner's word | [decisions](docs/agents/decisions.md) |
| retired number | A decision number that is missing (31, 53, 55) because the item was removed | [decisions](docs/agents/decisions.md) |
| `scripts/loop/checks NN` | The loop's per-cycle check, run by criterion number | [loop](docs/agents/loop.md) |

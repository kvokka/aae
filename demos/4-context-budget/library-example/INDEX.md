# Documentation index

## `docs/README.md` — Documentation
- **Using aae:** Guides readers to the product, usage guides, API references, and demos.
- **How it works inside:** Points to the architecture and design references for the daemon and its adapters.
- **Working on aae:** Lists developer guidance, acceptance criteria, and repository agent rules.

## `docs/acceptance.md` — Acceptance scenarios
- **Criteria and run modes:** Explains which end-to-end scenarios run in CI or on request and what each command needs.
- **Criteria:** Defines the numbered product, performance, recovery, integration, and release acceptance checks.

## `docs/agents/README.md` — For agents working on this repository
- **Working rules:** Directs agents to the relevant code, test, decision, assumption, and loop documents.
- **What is true:** States that code and current docs define the product and work comes from the owner.
- **Language:** Sets English for product materials and Russian for loop files.

## `docs/agents/assumptions.md` — Assumptions and open questions
- **Waiting for the owner:** Records unresolved choices about environment references.
- **Product shape:** Lists assumed boundaries and behavior of the daemon, state, events, and adapters.
- **Config:** Records configuration layout, merging, reload, and reference assumptions.
- **Harnesses:** Describes driver and session limitations.
- **Agents, packs, formulas:** Records assumed behavior for prompts, scopes, steps, checks, and pack loading.
- **Runs and triggers:** Lists run lifecycle, concurrency, cancellation, correlation, and trigger assumptions.
- **Tracker:** Describes assumed bead input, ownership, labels, rig prefixes, and failure gates.
- **Addons and frontends:** States hook decision and MCP behavior assumptions.
- **Delivery and process:** Records installation, service, cache, and budget assumptions.
- **Not verified:** Lists behaviors that still need evidence before relying on them.
- **Risks the owner accepted:** Pairs accepted product and vendor risks with their mitigations.

## `docs/agents/decisions.md` — Owner decisions
- **Decisions:** Records settled product choices that should not be reopened without the owner's direction.

## `docs/agents/loop.md` — Autonomous work loop
- **Loop operation:** Describes the journal, plan, expert roles, br workflow, and timeouts used by the repository loop.

## `docs/agents/rules-code.md` — Code rules
- **Budgets and implementation rules:** Sets code size, no-stub, measurement, dependency, and Git requirements.

## `docs/agents/rules-tests.md` — Test rules
- **Testing rules:** Explains what to run, when model-backed checks are allowed, and how tests and demos are handled.

## `docs/api/frontends.md` — Frontends
- **HTTP API and CLI:** Describes how people inspect and control runs through the API and command line.

## `docs/api/mcp.md` — MCP
- **MCP server:** Documents the MCP interface and tools exposed by aae.

## `docs/architecture.md` — Architecture
- **System structure:** Explains component boundaries, crates, and how a run moves through the system.

## `docs/design/adapters.md` — Harness adapters
- **Driver contract:** Defines what the core expects from harness adapters.
- **Adding a harness:** Describes the adapter work needed to support another harness.
- **Contract tests:** Explains how adapter protocol behavior is checked.

## `docs/design/config.md` — Configuration
- **Configuration files and loading:** Describes the orchestrator and project files and validation/reload behavior.
- **Settings and layers:** Defines precedence and merging across config layers.
- **Packs, agents, and variants:** Covers reusable project content and effective agent configuration.
- **Formulas and steps:** Specifies workflow inputs, step kinds, and execution settings.
- **Triggers and addons:** Documents event/schedule starts and addon declaration and attachment.
- **Reference and permissions:** Defines template, environment, native harness, and permission settings.

## `docs/design/contracts.md` — Contracts
- **Events and schemas:** Defines the event records and their stable fields.
- **Filters and templates:** Describes event matching and prompt interpolation.
- **Orchestrator hooks:** Specifies hook inputs, decisions, ordering, and timeouts.
- **Driver interface:** Sets the contract between core and harness adapters.
- **Tracker interface:** Defines tracker operations and errors.
- **Runs and transitions:** Describes lifecycle states, attempts, outcomes, gates, and recovery decisions.
- **Registry and policy:** Covers registered projects and the bundled default policy.

## `docs/design/event-model.md` — Event model
- **State and journal:** Explains the single-writer state model and append-only event journal.
- **Deadlines and delivery:** Describes scheduling, outbox delivery, and event-driven work.
- **Resynchronization:** Explains how external state is reconciled and ambiguity handled.

## `docs/design/experiments.md` — Experiments
- **Harness experiments:** Reports measurements and protocol checks on real harnesses.
- **Limits and findings:** Records what was verified and what remains uncertain.

## `docs/design/invariants.md` — Core invariants and recovery
- **The invariants:** Lists numbered core guarantees and how automated scenarios check them.
- **Recovery:** Describes startup recovery, planned drain/restart, and delivery guarantees.
- **Limits:** Notes platform behavior not checked in the first release.

## `docs/design/tracker-br.md` — The br tracker
- **External binary and license:** Explains br installation, separation, and license notice.
- **One database per project:** Describes tracker discovery, actors, and database boundaries.
- **Beads and runs:** Covers task inputs, ownership, claims, ready selection, and run outcomes.
- **The rig of a bead:** Defines rig prefixes and how sessions create correctly scoped beads.
- **Generated from project configs:** Describes environment and bv workspace configuration.
- **Limits of br:** Documents adapter constraints and br-specific behavior.
- **Exit codes:** Explains error handling and retry classification.
- **Versions:** Describes tested versions and adapter contract tests.

## `docs/guide/addon-development.md` — Writing an addon
- **The manifest:** Introduces addon files, parameters, and settings.
- **Values filled in:** Defines addon parameter and directory substitutions.
- **The agent side:** Describes settings, hooks, services, and prompt additions delivered to sessions.
- **The orchestrator side:** Specifies hook execution, decisions, ordering, and idempotency guidance.
- **A worked example: context-budget:** Shows an addon that tracks and limits session compactions.
- **Testing an addon:** Describes config validation and manually exercising hooks.
- **Moving it to its own repository:** Explains publishing and pinning an addon from Git.

## `docs/guide/addons.md` — Using addons
- **Declare it:** Shows local and Git-based addon declarations.
- **Attach it:** Explains settings-layer attachment, ordering, and disabling.
- **Parameters:** Describes parameter defaults and overrides.
- **Harnesses:** Explains compatibility checks for addon target harnesses.
- **Check what an agent gets:** Uses config check and explain to inspect effective addon settings.
- **Subagents:** Describes native subagent configuration and inherited settings.
- **What is left to the harness:** Lists nesting, built-in agent types, and codex-specific controls.

## `docs/guide/development.md` — Developing aae
- **Toolchain and tasks:** Lists pinned developer tools and the mise commands for checks and builds.
- **Where each kind of test runs:** Compares test, CI, acceptance, and release checks with their purpose and costs.
- **The chaos stand:** Explains randomized invariant testing and seed replay.

## `docs/guide/first-formula.md` — Your first formula with an agent
- **The agent:** Creates a role and explains its settings and prompt files.
- **The formula:** Builds an AI workflow and explains template validation.
- **How a run ends: the finisher:** Shows explicit outcome reporting and permissions needed to call it.
- **Run it:** Starts and inspects a run, including waits and gates.
- **Several steps:** Demonstrates ordered steps, prior results, checks, and returns.
- **A worktree per run:** Explains isolated worktrees and cleanup.
- **Runs over a tracker:** Shows task beads, planning, drain execution, and child formulas.

## `docs/guide/first-project.md` — Your first project
- **The orchestrator config:** Sets the daemon's projects and optional harness defaults.
- **The project:** Creates a project root and explains rigs.
- **A formula without a model:** Defines a simple exec-based workflow.
- **Start the daemon and run it:** Shows basic CLI inspection and event commands.
- **Change the config:** Explains validation and explicit reload.
- **Triggers:** Starts workflows from schedules or events.

## `docs/guide/install.md` — Installing aae
- **Build it:** Gives source build and multi-platform release build instructions.
- **What aae runs:** Lists external harness and tracker dependencies and their setup.
- **Check the setup:** Describes `aae doctor` checks and output.
- **Directories:** Lists XDG config, state, socket, and cache locations.
- **Run the daemon:** Explains service manager setup, drain, stop, and restart.

## `docs/guide/recovery.md` — Recovering a failed run by hand
- **See what failed:** Shows how to inspect failure class, attempt details, bead, and gate state.
- **Continue the session:** Starts a new run on the prior harness session.
- **Start a new session on the same task:** Retries the bead with a fresh session.
- **Start a new session with a note:** Adds prior-work context through a dedicated formula and agent.
- **Abandon it:** Cancels a live run or closes a bead gate to prevent another attempt.

## `docs/harness-updates.md` — Harness updates
- **Updating a harness:** Gives the version update, contract test, real turn, and tested-range workflow.
- **How the suite is built:** Describes fake-provider scenarios and their coverage.
- **What the claude suite covers:** Lists Claude adapter expectations and daemon integration checks.
- **What the codex suite covers:** Lists Codex-specific protocol expectations.
- **Log:** Records tested harness versions and outcomes.

## `docs/overview.md` — aae in one page
- **What shapes it:** Summarizes design principles and supported builds.
- **Glossary:** Defines the main product terms.
- **What the first release leaves out:** Lists deferred or excluded capabilities and current alternatives.

## `docs/roadmap.md` — Roadmap
- **Planned extensions:** Lists possible addon-based metrics, budgets, compaction, notices, recovery, and permission presets with dependencies and priority.

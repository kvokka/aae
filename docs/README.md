# Documentation

Start with [loop-manager in one page](overview.md) and the [architecture](architecture.md); after that, read by need.
The big references (`design/contracts.md`, `design/config.md`) are meant to be read by section.

## Using loop-manager

| Document | What is in it |
|---|---|
| [overview.md](overview.md) | what loop-manager is, the glossary, what the first release leaves out |
| [roadmap.md](roadmap.md) | what lies ahead as addons; a hint, not a work list |
| [guide/install.md](guide/install.md) | installing, what loop-manager runs, the daemon as a service, upgrading |
| [guide/first-project.md](guide/first-project.md) | a project, its root and rigs |
| [guide/first-formula.md](guide/first-formula.md) | a formula with an agent, from the config to a passed run |
| [guide/recovery.md](guide/recovery.md) | a run that failed for good: what failed, continue the session, a new session with or without a note, abandon |
| [guide/addons.md](guide/addons.md) | using addons: declare, attach, parameters; an agent's subagents |
| [guide/addon-development.md](guide/addon-development.md) | writing an addon: the manifest, the agent side and the orchestrator side, tests |
| [design/config.md](design/config.md) | every config file and key; layers and "explain config"; packs, agents, formulas, triggers, addons |
| [api/frontends.md](api/frontends.md) | the HTTP API and the CLI; what a human sees and does with a run |
| [api/mcp.md](api/mcp.md), [api/openapi.json](api/openapi.json) | the MCP server; the OpenAPI document |
| [../demos/README.md](../demos/README.md) | two live examples |
| [../devloop/README.md](../devloop/README.md) | the pack that develops loop-manager with loop-manager |

## How it works inside

| Document | What is in it |
|---|---|
| [architecture.md](architecture.md) | the parts, the boundaries, the crates, the life of a run |
| [design/event-model.md](design/event-model.md) | the single writer, the state, the journal, deadlines, resync |
| [design/invariants.md](design/invariants.md) | the invariants of the core, recovery, known limits |
| [design/contracts.md](design/contracts.md) | events, the filter and template language, hooks, the driver and tracker interfaces, run states, gates, the registry, the default policy |
| [design/adapters.md](design/adapters.md) | harness adapters: what the core expects, adding a harness, the contract suite |
| [harness-updates.md](harness-updates.md) | updating claude or codex, the contract scenarios |
| [design/tracker-br.md](design/tracker-br.md) | the br tracker: beads and runs, rig prefixes, the limits of br |
| [design/experiments.md](design/experiments.md) | what was measured on the real harnesses, and what was not |

## Working on loop-manager

| Document | What is in it |
|---|---|
| [guide/development.md](guide/development.md) | the toolchain, the tasks, where each kind of test runs, the chaos stand |
| [acceptance.md](acceptance.md) | the end-to-end scenarios and their criteria; run on request |
| [agents/](agents/README.md) | the rules for agents working on this repository: code, tests, decisions, assumptions, the loop |

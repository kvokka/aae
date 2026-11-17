# Architecture

How the parts of aae fit together and where each lives. The terms are in the [glossary](overview.md#glossary).

```
            HTTP API (full)    CLI (full)    MCP (small)
                    └──────────────┼──────────────┘
                           frontend adapters
                                   │  commands / queries / event stream
┌──────────────────────────────────▼───────────────────────────────────────┐
│ CORE: one writer of the state                                            │
│  input queue → handling in an SQLite transaction → events (outbox)       │
│  runs and attempts · triggers and timers · config assembly · instances   │
│  exec hooks of plugins · bead ownership · recovery and resync            │
└──────┬─────────────────────┬─────────────────────┬───────────────────────┘
       │ driver interface    │ tracker interface   │ events → JSONL journal
  harness adapters       br adapter            plugins (exec hooks, services)
  claude │ codex
```

## Boundaries

These are checked by the build, not kept by agreement.

- The core is two crates: `aae-contract`, the contracts adapters, the tracker adapter, plugins and test doubles are
  written against, and `aae-core`, the engine over them. The core depends on no adapter, frontend or plugin, and
  neither crate names a harness or a tracker (`codex`, `br`, `max_sessions`, `BEADS_DIR`, `.beads`): what the core
  needs comes through the driver's capabilities and the tracker interface, and `scripts/core-boundary.sh` checks the
  sources of both in CI, and that the adapters (`aae-claude`, `aae-codex`, `aae-br`, `aae-plugins`) build without
  `aae-core`.
- A harness is an abstraction to the core. The root of the build holds the one table of adapters
  (`crates/aae/src/harness.rs`), the only place that names a harness.
- Adapters and frontends only translate protocols.
- Policy (retry, park, refuse) lives in plugins and config, judgement ("is the work done well") in prompts and the
  checks of plugins; the core keeps only mechanisms
  ([where the bundled plugins end](design/config.md#where-the-bundled-plugins-end-and-the-core-begins)).

## Crates

| Crate | What it is | Read |
|---|---|---|
| `aae-contract` | the contracts of the core: config assembly, the driver and tracker interfaces, the event envelope, templates and schedules, the registry of child processes. The adapters, `aae-br`, `aae-plugins` and the test doubles depend on it, not on the engine | [contracts](design/contracts.md), [config](design/config.md) |
| `aae-core` | the engine, the single writer of the runtime state: runs, attempts, triggers, deadlines, gates, the instance registry, hook execution, recovery. It re-exports the modules of `aae-contract` | [event model](design/event-model.md), [invariants](design/invariants.md) |
| `aae-claude` | the claude adapter: one `claude -p` process per session, stream-json | [adapters](design/adapters.md), [contracts 4.6](design/contracts.md#46-harness-specifics-the-contract-relies-on) |
| `aae-codex` | the codex adapter: one `codex app-server` per fingerprint over stdio, a thread per session | the same |
| `aae-br` | the br tracker adapter: every call is one `br` process on the project's database | [tracker](design/tracker-br.md) |
| `aae-front` | the frontends: the HTTP API on the daemon's unix socket, the CLI and its client, the MCP server | [frontends](api/frontends.md), [MCP](api/mcp.md), [OpenAPI](api/openapi.json) |
| `aae-plugins` | the bundled plugins `default-policy` and `worktree`, in-process executors of the hook contract | [config: bundled plugins](design/config.md#bundled-plugins-builtinname), [contracts 10](design/contracts.md#10-default-policy-as-decisions) |
| `aae` | the binary: the daemon, the adapter table, `doctor`, the git cache of packs and plugins, the stdio→HTTP MCP bridge | [install](guide/install.md) |
| `aae-testkit` | test doubles: the scripted driver, fake providers, a test MCP service. Nothing of it runs in the product | [development](guide/development.md) |
| `aae-acceptance` | end-to-end scenarios on a real daemon and real binaries, run on request | [acceptance](acceptance.md) |

aae's own plugins are not crates either: each lives in `plugins/<name>/` of this repository and is self-contained —
its `plugin.toml`, a `README.md`, its own files and its own version — so moving one to a repository of its own
changes only its declaration. A config declares a plugin by `[[plugin]]` with a `path`, or with `git` + `rev` + `dir`
for a directory of a repository ([config: packs and plugins from git](design/config.md#packs-and-plugins-from-git)),
and an agent attaches it with `plugins = [{ name, params }]`
([config: plugin](design/config.md#plugin-pluginplugintoml)). There is one so far:
[`context-budget`](../plugins/context-budget/README.md), the compaction window of claude and codex sessions and a
budget of compactions per session.

The skills for a user's agent are a different thing: `operator-plugin/` is a plugin of the operator's harness, Claude
Code or Codex, with the skills by which the operator drives aae, each harness in its current format. Claude Code reads
`.claude-plugin/marketplace.json` and the plugin's `.claude-plugin/plugin.json`; Codex reads
`.agents/plugins/marketplace.json` and the plugin's root `plugin.json`
([recovery](guide/recovery.md)). A test of `aae-front` checks its manifests and that its skills name only commands
and flags the CLI has.

## The life of a run

1. **A start.** A run starts by an explicit call (API, CLI, MCP), by a subscription to an event or by a schedule
   ([triggers](design/config.md#trigger)). The start names a project, a formula and its inputs; the task either names
   an existing bead or becomes a new root bead ([tracker](design/tracker-br.md)).
2. **One writer.** Everything that happens enters the core as a message to the single writer and is handled in one
   SQLite transaction together with the events it emits; effects run after the commit
   ([event model](design/event-model.md)).
3. **Steps in order.** The run goes through the steps of its formula: `ai`, `exec`, `drain`, `formula`, `await`
   ([formula](design/config.md#formula-formulasfiletoml)). For each step the core assembles the effective config from
   its layers ([config](design/config.md#effective-config-and-explain-config)).
4. **A session.** For an `ai` step the lazy registry gives a harness instance for the step's fingerprint, and the
   adapter starts a session in it ([contracts 8](design/contracts.md#8-instance-and-service-registry)).
5. **An outcome.** The agent reports `pass` or `fail` with the finisher; a turn that ends without one is a failure,
   not a pass. Hooks on `run.finishing` may veto or send a remark back
   ([contracts 6.3](design/contracts.md#63-finisher)).
6. **Failures.** An attempt that fails (a harness crash, a limit, a stall) goes to the policy plugin, which resumes,
   parks on a gate or ends the run ([contracts 10](design/contracts.md#10-default-policy-as-decisions),
   [gates](design/contracts.md#7-issue-gates)).
7. **Restarts.** After a daemon restart the core reads its state, stops what was left running and resumes each run
   from the step it was at ([invariants: recovery](design/invariants.md#recovery)).

The states of a run and their deadlines are the table of [contracts 6.1](design/contracts.md#61-states).

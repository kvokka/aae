# Using addons

An addon is a directory with an `addon.toml`. It gives the agents that attach it settings (skills, harness hooks,
MCP servers, services, environment variables, native harness keys, a prompt fragment) and gives the orchestrator hooks
on the events of their runs. An addon is used in two steps: it is **declared** once, then **attached** where it
should work. This guide uses [`addons/context-budget`](../../addons/context-budget/README.md) of this repository,
which bounds the context of a claude or codex session. To write an addon, see
[writing an addon](addon-development.md); the full rules are in
[the config reference](../design/config.md#addon-addonaddontoml).

The [conventional-commits addon](../../addons/conventional-commits/README.md) checks Git commit subjects and appends a format hint to the coding agent's prompt.

## Declare it

A declaration only says where the addon is. It goes in the orchestrator config or in a project's `loop-manager.toml`:

```toml
# ~/.config/loop-manager/config.toml
projects = ["~/src/shop"]

[[addon]]
path = "~/loop-manager-addons/context-budget"

[settings]
addons = [{ name = "context-budget" }]
```

An addon in a git repository is declared with `git` and `rev` instead of `path`, and with `dir` when it is not at the
root of the repository:

```toml
[[addon]]
git = "https://github.com/acme/loop-manager"
rev = "v1.4.0"                      # a tag or a full commit id; required for an addon
dir = "addons/context-budget"
```

`loop-manager` downloads it into its cache when it loads the config and runs it only at that tag or commit
([packs and addons from git](../design/config.md#packs-and-addons-from-git)). Addon names are unique across the
orchestrator and the project. `loop-manager addon list` lists the declared addons and the bundled ones.

## Attach it

Declaring turns nothing on: an addon works only where `addons` attaches it. `addons` is a key of every settings
layer, an array of `{ name, params, enabled }`:

| Where | Reaches |
|---|---|
| `[settings]` of the orchestrator or a project | every agent there; also the events of no agent (`bead.*`, `service.*`, exec steps) |
| `[rigs.<rig>.settings]` | the agents on that rig |
| `agent.toml`, a variant `[agents.<name>]`, an edit `[agents.<agent>]` or `[rigs.<rig>.agents.<agent>]` | that agent |

The config above attaches `context-budget` to every agent. An agent attaches one for itself the same way:

```toml
# ~/src/shop/agents/dev/agent.toml
harness = "claude"
subagents = ["qa"]
addons = [{ name = "context-budget", params = { soft = 200000 } }]
```

An agent has any number of addons. They apply in the order of their first mention, the layers read weakest first
(orchestrator, project, rig, agent, variant, edits); a stronger layer that names an addon again keeps its place. A
stronger layer detaches an addon with `{ name = "context-budget", enabled = false }`, and with it the addon's whole
contribution to that agent. An addon's agent side sits between the rig settings and the agent's own `agent.toml`, so
an agent can still override what an addon sets.

### Parameters

A parameter takes its default from the addon's `[params]`, then the value each layer that attaches the addon gives,
key by key, the strongest last. In the example `dev` gets `soft = 200000` from its own attachment and
`hard = 420000` from the addon. A key the addon does not declare, or a value of another type than its
default's, is a load error of the file that has it.

### Harnesses

An addon may name the harnesses it is for (`harnesses = ["claude", "codex"]`). An agent on another harness that
attaches it is a load error; so is a subagent target whose addon is not for the caller's harness.

### Check what an agent gets

```sh
loop-manager config check
loop-manager config explain --project shop --agent dev
```

`explain` prints each value with the layer it came from: what an addon adds is marked `# addon <name>`, and each
parameter is a line `addons.<name>.params.<key>` with the layer that set it. Both commands take `--config <file>` to
load a config without a daemon.

## Subagents

An agent's session may call other agents of the project as native subagents of its harness. A subagent is not a run:
it has no finisher, gates, `max_runs` or orchestrator hooks. Two keys of `agent.toml`, a variant or an edit (not of a
`[settings]` table) set it up:

- `subagents`: the agents and variants the session may call, by name (`qa`, `kit.qa`, a variant's name) or by
  pattern (`qa-*`, `kit.*`, `*`). The agent itself is always in the list.
- `description`: what the agent does. A target needs one; it is what the caller's model reads to pick a subagent.

```toml
# ~/src/shop/agents/qa/agent.toml
harness = "claude"
description = "Checks a change: runs the tests and reads the diff."
```

A target's subagent gets the target's own config: its role prompt (with its addons' fragments), model, effort,
skills, and on claude the MCP servers and hooks the session has not (a codex subagent has the session's only, so a
codex target with others is a load error). Its `env` and native block do not reach it, since it lives in the
caller's process. A target must be on the caller's harness and fit it in other ways; what does not fit is a
load error of the project ([subagents](../design/config.md#subagents)). The set of targets is one per session: a
nested subagent calls within it.

`loop-manager agent list` (and `GET /v1/agents`) shows the call graph of each agent: `subagents`, the agents it may
call, and `called_by`, the agents that may call it.

### What is left to the harness

- **Nesting.** claude goes two levels deep unless `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` in the agent's `env` says
  otherwise. codex's depth is its own setting, below.
- **Built-in types.** claude's built-in agent types (`general-purpose`, `Explore`, `Plan`) are removed, so a claude
  session calls only its targets and claude's own `fork`. codex keeps its built-in types (`default`, `explorer`,
  `worker`). A target cannot take the name of a built-in type.
- **codex limits**, set in `native.codex` of the caller: `agents.max_threads` (open subagents, six by default),
  `agents.max_depth` (one level by default, in the older multi-agent version) and `features.multi_agent_v2 = true`,
  which forces the newer version (codex otherwise picks it by the model).

```toml
# a codex agent's agent.toml
[native.codex]
agents.max_threads = 3
features.multi_agent_v2 = true
```

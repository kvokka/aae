# aae in one page

`aae` (Adaptive Agent Executor) is an event-driven orchestrator for coding agents. One daemon per user of a machine
runs **formulas** on **agents** in the **rigs** (repositories) of **projects**, through the agents' own harnesses
(Claude Code, Codex), and takes every run to an explicit outcome through harness failures, account limits and its own
restarts.

## What shapes it

1. **A very light core.** The core holds only what a run needs to start, be observed and end correctly. Everything
   else is an adapter (harness, tracker, frontend) or a plugin.
2. **Events, not ticks.** No unit of work waits for a polling timer: the delay from a cause to the reaction depends
   on no interval. Safety timers (the orphan sweep, an instance health check, resync) exist and advance nothing in
   normal work.
3. **Native harness protocols, no tmux.** A terminal screen is never a control channel.
4. **Less code is better.** Only what a described result needs is written: no stubs, no extension points, no fields
   or flags for later. What is deferred is absent from the code altogether.

aae ships as one binary for Linux and macOS, x86_64 and arm64, under the MIT license. It is checked on Linux arm64
only; the other three targets are built and trusted to the portability of the language.

## Glossary

| Term | Meaning in aae |
|---|---|
| **orchestrator** | the `aae` daemon and its machine-level settings |
| **project** | a directory with `aae.toml`: a root, a set of rigs, packs, plugins and one tracker database |
| **root** | the root directory of a project: where agents of scope `root` work, and in a one-repository project the executors too. By default the directory of `aae.toml`, named `root`; both are settings. It need not be a git repository |
| **rig** | a repository of a project on disk, with its bead prefix and variables (`test_cmd`, `lint_cmd`, …) |
| **harness** | a kind of agent CLI: `claude`, `codex`. One adapter each |
| **harness instance** | a running harness process (an app-server or a session process) that serves one or several sessions |
| **agent** | a general role (`coder`, `planner`): a role prompt, a prompt template, skills, a scope and default launch settings. It knows nothing of a concrete task. The core's code names no role |
| **agent scope** | `rig`: the agent works in one rig's directory; `root`: in the root, over all rigs of the project |
| **launch settings** | three scalars: harness, model, effort |
| **model short name** | a family name (`opus`, `luna`) the adapter expands to the latest model of that family |
| **agent variant** | a pack's agent under another name and with other settings, declared in the user's config (`claude-coder`, `codex-coder`) |
| **session reference** | the pair "harness + the harness's session id": all the core knows of a conversation |
| **formula** | a workflow, the "how": a name, a description, inputs and steps in order. The "what", the task, comes only from the user |
| **step** | `ai` (an agent's turn), `exec` (a command, no LLM), `drain` (giving beads out to executors) or `formula` (another formula) |
| **task** | the text a user gives a formula at start: what to do. It names an existing bead, or becomes a new root bead |
| **run** | an execution of a formula with concrete inputs; it has attempts and an outcome |
| **child run** | a run started by a `drain` or `formula` step of another run, or from its agent's session; it inherits that run's correlation |
| **attempt** | one start or resume of a session within a run |
| **finisher** | the call by which an agent reports the outcome of its step |
| **trigger** | a rule "an event or a schedule → start a formula" |
| **correlation** | the id of a chain of runs that started one another |
| **pack** | content fit for any project: agents, formulas, triggers |
| **plugin** | an extension: what attaches to agents (skills, hooks, MCP, scripts, env), hooks on orchestrator events, services |
| **service** | a long-lived process the orchestrator supervises (a shared MCP server, for one) |
| **issue gate** | a record "start no run by this key until a time, or until opened by hand" |
| **tracker** | the task store behind an adapter interface; the first implementation is br |
| **bead** | a task in the tracker. Real work only, no bookkeeping records |
| **root bead** | the bead with the user's task; every bead filed for that task lies under it |
| **event** | a record of what happened: a reason to re-read the state, not the truth |

## What the first release leaves out

| Not included | Why, and what there is instead |
|---|---|
| Molecules, wisps, convoys, control beads | Chains are built from formula steps, event subscriptions and child runs |
| An arbitrary step graph inside a formula: branches, nested steps, parallel branches | A formula is a linear list of steps with a bounded return to an earlier step. A planner agent splits the work: it files beads, and a `drain` step gives them to executors |
| Agents that live permanently | A session per run |
| Mail and nudges between agents | A separate plugin, later |
| Storage of sessions and transcripts | The harness's job; the core keeps the reference "harness + id" |
| opencode | The next release, after a spike ([experiments](design/experiments.md)) |
| pi and other harnesses | Not planned |
| ACP as the transport "down" | Native protocols; the driver interface follows the semantics of ACP v2, so an ACP driver can be one more implementation |
| A human joining a live session, handover of control | A later release: no operations, states or stubs for it now. A human sees the session reference in the run's state and opens the conversation with the harness's own tools after the run ([frontends](api/frontends.md)) |
| An ACP hub "up" for editors | Not planned |
| A human gate as a formula step | A permission request and an operator hold do that job |
| A web interface, a dashboard | The task board is `bv`; observation is the CLI and the event stream |
| Alerting a human about a failure | Only the event journal and the logs; a notice plugin is on the [roadmap](roadmap.md) |
| Windows, several machines, several users | A local daemon of one user |
| Checks on macOS and on x86_64 | Checked on Linux arm64 only; the other targets are built |
| A bundled check "the work is really done" | A check is a formula step, a script or an agent, written by the author of the pack or the project |
| Account rotation and failover between subscriptions | A zone of vendor-policy risk; not the core's business |
| Translation of hooks between harness dialects | The hook formats of claude and codex are nearly one; a translator when a third dialect appears |
| A generic ACP adapter, one adapter for several harnesses | When a third harness appears |
| A model provider setting apart from the harness | The user names a harness; the provider inside it is the harness's native setting |
| A chain of several formulas over one bead | `pass` closes the bead; the next work comes by taking the next ready bead |
| A demo "aae develops itself" | Later: the agent setup of this repository as an aae pack |
| Migration of data and packs from other orchestrators | A separate task |

Next: [architecture](architecture.md), then the [guides](guide/install.md).

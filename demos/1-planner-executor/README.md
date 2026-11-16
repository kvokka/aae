# Demo 1: a planner and its executors

What a harness does with subagents, over two repositories: a planner splits one request into tasks, files them as
beads, and coders do them, each in its own repository. Nothing more: no QA, no check of the result as a whole.

The request is one sentence: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

- `pack/` is the pack `dev`, with not a word about the task. Agent `planner` files the work; agent `coder` does one
  bead. Each agent is `agents/<agent>/agent.toml` and the prompt its steps get, `agents/<agent>/prompt.template.md`;
  its `scope` says where it works: `root` (the planner, over all the rigs) or `rig` (the coder, in its bead's rig).
  Formula `deliver`:
  1. `plan`, an ai step of `planner`: it looks at the rigs and files beads under the request, each with its task
     and the labels `agent:` and `harness:`, under the prefix of its rig;
  2. `work`, a `drain` step: every ready bead under the request gets a run of `implement`, where `coder` does it on
     the rig of the prefix of its id, two beads at once (`pool = 2`);
  3. `report`, an exec step: the beads as they are and whether every bead under the request is closed, with errors or
     without.
- `root/` is the root repository: `aae.toml` (the project, its rigs and what the agents run on) and its tracker, the
  beads, made by the run. `root/config.toml` is the orchestrator config of the demo: `run.sh` starts a daemon of the
  demo's own with it, so the demo neither reads nor changes your `~/.config/aae`.
- `frontend/` and `backend/` are the two repositories of the project, empty before a run.

## Run

You need `aae`, logins of `claude` and `codex`, `br` and `jq`; `mise install` in this directory installs all but
`aae` (mise is a convenience, any install does). The run spends real tokens on the cheapest models.

```sh
aae config check --config root/config.toml   # the pack and the project load
./run.sh                                     # or: mise run demo
```

`run.sh` takes a few minutes and writes `RESULT.md`: when and on what it ran, the bead tree, the runs and the
outcome. Everything a run creates is in `.gitignore`; `git clean -ffdX -- .` clears it for the next run. Compare
with the author's run: `frontend-example/`, `backend-example/` and `RESULT-example.md`, with its date, versions and
models. Your result differs: the agents decide anew each time.

## The harness of a coder

There is one agent `coder`; what it runs on is the user's config, not the pack's. `root/aae.toml` runs it on claude
`haiku` in the whole project and on codex `luna` on the rig `backend`, with the rig's settings for that agent:

```toml
[rigs.backend.agents.coder]
harness = "codex"
model = "luna"
```

The permission mode is each harness's own setting, so it is in the harness's native block of `[settings]`:
`permission_mode` for claude, `approval_policy` for codex.

The other way is a variant per harness, mapped on the rig in place of `coder`:

```toml
[agents.claude-coder]
base = "dev.coder"
harness = "claude"
model = "haiku"

[agents.codex-coder]
base = "dev.coder"
harness = "codex"
model = "luna"

[rigs.frontend]
path = "../frontend"
agents = { coder = "claude-coder" }
[rigs.backend]
path = "../backend"
agents = { coder = "codex-coder" }
```

A variant is an agent of its own name: its runs, gates and `max_runs` are apart from `coder`'s. Either way,
`vars.harness` of a rig tells the planner what to put in the `harness:` label of a bead; a bead whose label
disagrees with the config gets no run. `aae config explain --config root/config.toml --project planner-executor
--agent dev.coder --rig backend` shows what a coder gets and from which layer.

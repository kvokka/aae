# Demo 4: a context budget for an agent

What the addon [`context-budget`](../../addons/context-budget/README.md) does: an agent on claude reads about 100000
tokens of documents, more than the budget the addon gives it. At the budget's soft limit the addon tells the agent to
wrap up; the agent stops reading, leaves a summary of its work as a comment on the bead of its run and finishes the
run.

There are two requests, one per agent, run side by side:

- "List every term the documents in docs/ define, each with a one-line definition and the document that defines it,
  in GLOSSARY.md";
- "Write INDEX.md: for each document in docs/, its sections, each with one line on what it says".

The documents are the `docs/` of this checkout of loop-manager, copied by `run.sh`.

- `pack/` is the pack `study`, with not a word about the addon. Agent `glossarist` runs on claude, agent `indexer`
  on codex; each has the prompt its step gets, `agents/<agent>/prompt.template.md`: read every document to its end,
  write the one file the request asks for. Formula `glossary` is one ai step of `glossarist`, formula `index` one of
  `indexer`.
- `root/` is the project `library`: `loop-manager.toml` (its pack and what the agents run on) and its tracker, the beads, made
  by the run. The project has no rigs; its root is `library/`, where `run.sh` copies the documents and the agents
  write. `root/config.toml` is the orchestrator config of the demo: `run.sh` starts a daemon of the demo's own with
  it, so the demo neither reads nor changes your `~/.config/loop-manager`.

The addon serves claude only; there is none for codex yet, so `indexer` runs without it and reads all the documents.

## Run

You need `loop-manager`, logins of `claude` and `codex`, `br` and `jq`; `mise install` in this directory installs all but
`loop-manager`. The addon's hook calls `jq`, `br` and `loop-manager` too. The run spends real tokens on the cheapest models: claude
`haiku` and codex `luna`.

```sh
loop-manager config check --config root/config.toml   # the pack, the project and the addon load
./run.sh                                     # or: mise run demo
```

`run.sh` writes `RESULT.md`: when and on what it ran, and for each agent what it ran on with the addon's params and
the outcome of its run, the comments on the bead of `glossarist`, and the files the addon kept. The demo passes when
both runs pass and the bead of `glossarist` carries a comment. Everything a run creates is in `.gitignore`;
`git clean -ffdX -- .` clears it for the next run. `library-example/` holds what the agents wrote in a run of the
author's.

## How the addon is set up

`root/config.toml` declares the addon by its directory in this repository:

```toml
[[addon]]
path = "../../../addons/context-budget"
```

The project attaches it to `glossarist` alone, in `root/loop-manager.toml`. The agent runs on `haiku`, with a window
of 200000 tokens:

```toml
[agents.glossarist]
model = "haiku"
addons = [{ name = "context-budget", params = { soft = 40000, hard = 150000, warn_step = 20000 } }]
```

- `soft = 40000`: past 40000 tokens of context the agent gets a note after a tool call: its context, its budget and
  how to wrap up; the note comes again each time the context grows by `warn_step` (20000) tokens. It starts nothing new and calls no subagents, finishes or
  reverts the step at hand, commits, writes what is done, what is not and what is started and not finished as a
  comment on its bead, and finishes the run.
- `hard = 150000`: the safety net, of which the agent is told nothing. A session that gets there anyway is stopped:
  the addon writes a dump of its state (git status, the commits of the run, the diff stat, the last tool calls and texts
  of the agent) as a comment on the bead and finishes the run with `cut`, `fail` by default.
- The addon turns off claude's own compaction of the context, so the budget is the one limit the session meets.

`loop-manager config explain --config root/config.toml --project library --agent study.glossarist` shows each param
with the layer that set it.

## What to check

- `br comments list <bead>` in `root/` (the bead is in `RESULT.md`): the agent's summary of its work, written when it
  wrapped up.
- `.loop-manager/state/loop-manager/addons/context-budget/`: the files the addon keeps for each session,
  `<session>.*`.

# Demo 4: a context budget for every agent

What the plugin [`context-budget`](../../plugins/context-budget/README.md) does on both harnesses: an agent on claude
and an agent on codex each read about 100000 tokens of documents, more than the window the plugin gives them, so
each session compacts its context; the plugin counts the compactions and keeps the label `compactions:<n>` on the
bead of the run.

There are two requests, one per agent, run side by side:

- "List every term the documents in docs/ define, each with a one-line definition and the document that defines it,
  in GLOSSARY.md";
- "Write INDEX.md: for each document in docs/, its sections, each with one line on what it says".

The documents are the `docs/` of this checkout of aae, copied by `run.sh`.

- `pack/` is the pack `study`, with not a word about the plugin. Agent `glossarist` runs on claude, agent `indexer`
  on codex; each has the prompt its step gets, `agents/<agent>/prompt.template.md`: read every document to its end,
  write the one file the request asks for. Formula `glossary` is one ai step of `glossarist`, formula `index` one of
  `indexer`.
- `root/` is the project `library`: `aae.toml` (its pack and what the agents run on) and its tracker, the beads, made
  by the run. The project has no rigs; its root is `library/`, where `run.sh` copies the documents and the agents
  write. `root/config.toml` is the orchestrator config of the demo: `run.sh` starts a daemon of the demo's own with
  it, so the demo neither reads nor changes your `~/.config/aae`.

## Run

You need `aae`, logins of `claude` and `codex`, `br` and `jq`; `mise install` in this directory installs all but
`aae`. The plugin's hook calls `jq`, `br` and `aae` too. The run spends real tokens on the cheapest models: claude
`haiku` and codex `luna`.

```sh
aae config check --config root/config.toml   # the pack, the project and the plugin load
./run.sh                                     # or: mise run demo
```

`run.sh` writes `RESULT.md`: when and on what it ran, and for each agent what it ran on with the plugin's params,
the labels of its bead, the outcome of its run, and the count of each session. The demo passes when both runs pass
and both beads carry a `compactions:<n>` label. Everything a run creates is in `.gitignore`;
`git clean -ffdX -- .` clears it for the next run. Compare with the author's run: `library-example/` (what the
agents wrote) and `RESULT-example.md`.

## How the plugin is set up

`root/config.toml` declares the plugin by its directory in this repository and attaches it in `[settings]`, so it
reaches every agent of every project of the daemon:

```toml
[[plugin]]
path = "../../../plugins/context-budget"

[settings]
plugins = [{ name = "context-budget", params = { window = 100000, max_compactions = 8 } }]
```

claude takes a window of 100000 at the least and compacts at about 67000 tokens of it. codex takes any size, so the
project gives `indexer` a smaller window of its own in `root/aae.toml`; `max_compactions` stays the one of
`config.toml`:

```toml
[agents.indexer]
plugins = [{ name = "context-budget", params = { window = 50000 } }]
```

`aae config explain --config root/config.toml --project library --agent study.indexer` shows each param with the
layer that set it, and the native key it reaches: `autoCompactWindow` of claude, `model_auto_compact_token_limit` of
codex. A session past `max_compactions` fails its run with `context budget: <n> compactions`; set it to 0 to see
that.

## What to check

- `br label list <bead>` in `root/` (the beads are in `RESULT.md`): `compactions:<n>`, one label, the count of the
  session's compactions.
- `.aae/state/aae/plugins/context-budget/`: a `<session>.count` per session that compacted.

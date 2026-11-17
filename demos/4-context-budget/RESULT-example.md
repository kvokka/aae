# 4-context-budget: passed

<!-- MEASURE id=demo-4-context-budget date=2026-11-17 ver=aae-0.1.0@54c37ca-dirty,claude-2.1.326,codex-0.169.0 env=Linux-aarch64 src=demos/4-context-budget/run.sh note="passed in 356 s from the requests, n=1" -->

Run on 2026-11-17 with aae-0.1.0@54c37ca-dirty,claude-2.1.326,codex-0.169.0 (`Linux aarch64`): passed in 356 s from the requests.

## glossarist

    model = "haiku"  # project [agents]
    harness = "claude"  # agent
    plugins.context-budget.params.max_compactions = 8  # orchestrator
    plugins.context-budget.params.window = 100000  # orchestrator
    native.claude = {"autoCompactWindow":100000,"permission_mode":"bypassPermissions"}  # project + plugin context-budget

The request: "List every term the documents in docs/ define, each with a one-line definition and the document that defines it, in GLOSSARY.md".

The labels of its bead `root-mm0` (`br label list`):

```
Labels for root-mm0:
  compactions:5
```

The outcome of the run (`aae run wait`):

```
id: run_1
reason: pass
state: passed
summary: GLOSSARY.md: every term the docs/ Markdown files define, in four tables (product and overview; architecture and event model; contracts, adapters and tracker; config, runs, recovery, frontends, rules and loop), each with a one-line definition and the defining document with section links. Built from a full read of all 29 docs files.
```

## indexer

    model = "gpt-6-luna"  # agent, given as "luna"
    harness = "codex"  # agent
    plugins.context-budget.params.max_compactions = 8  # orchestrator
    plugins.context-budget.params.window = 50000  # project [agents]
    native.codex = {"approval_policy":"never","model_auto_compact_token_limit":50000}  # project + plugin context-budget

The request: "Write INDEX.md: for each document in docs/, its sections, each with one line on what it says".

The labels of its bead `root-561` (`br label list`):

```
Labels for root-561:
  compactions:3
```

The outcome of the run (`aae run wait`):

```
id: run_2
reason: pass
state: passed
summary: INDEX.md maps every docs Markdown document to its sections with a one-line description of each.
```

The compactions the plugin counted, per session (`<state>/aae/plugins/context-budget/<session>.count`):

```
01a1eb27-3976-7f42-8c6f-7ce99c619341: 3
4c7ad33d-c987-573f-9296-5f37d5dc5377: 5
```

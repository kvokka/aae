# Docs sweep: config surface vs spec decisions 89-104 and code

Findings: 2 (both in docs/design/config.md, minor wording). No dead relative links or anchors (checked every
markdown link in the swept files with a script).

1. docs/design/config.md:552 — "with the bead as the formula's `bead` input"
   Old term. The input is identified by TYPE `task` (any name); code: crates/aae-core/src/config/check.rs:211-213
   ("needs exactly one input of type `task`"), spec decision 92/94, §9.1.
   Fix: "with the bead as the formula's `task` input".

2. docs/design/config.md:569 — "`formula` must take exactly one `bead` input and need no other; a formula with a
   `drain` step needs a required `bead` input."
   Same. Fix: "`formula` must take exactly one input of type `task` and need no other required input; a formula with
   a `drain` step needs a required `task` input."

Minor, optional:
3. docs/design/config.md:570 — "(with an id of no rig: in the root)" reads like the removed `rig:` label.
   Fix: "(an id with no rig prefix: in the root)". Same phrasing at line 552 "no rig named" is fine.
4. docs/design/config.md:426 — trigger `inputs = { bead = {...} }`: valid (input named `bead` of type `task`), but
   a reader may take it for the old type. Optional: name it `task` for consistency with the guides/demos.

Checked and clean: README.md; docs/guide/* (first-formula: kind ai/exec/drain/formula, pool, on_item_failure,
max_attempts, step_check_script_path, internal, [native.claude] permission_mode, prompt.template.md; first-project:
[root] optional, [harness.*] optional); docs/design/{config,contracts,adapters,tracker-br,invariants,event-model}.md
(old keys appear only in the "retired key" sentence config.md:20-22; `rig:` label only as "refused"); docs/api/*.md
(--all, internal, list_formulas); demos/README.md and demos/*/README.md plus demo TOMLs (scope, pool,
[settings.native.*], type = "task"); examples/aae-plugin-review (plugin.toml, `prompt = "prompt.md"` is the plugin
key, not a step key). The `examples/*.rs` in crates/aae-core/src/config are source files, not examples.
Not done: full TOML-vs-serde parse of every fenced block in config.md (772 lines); only keys were cross-checked.

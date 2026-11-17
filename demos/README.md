# Demos

Small live examples of aae (criterion 20), each in a directory of its own. The user's request is one
sentence; how to do it and in what order the agents decide. They run real models, so they need the harness logins
and spend tokens; CI only checks that each demo's pack and project load.

| Demo | What it shows |
|---|---|
| [`1-planner-executor`](1-planner-executor) | a planner files beads per repository, a `drain` step hands them to coders on claude and codex, a script reports what got done |
| [`2-multirepo-qa`](2-multirepo-qa) | demo 1 on openai models, then a format check and a qa agent in a browser send the work back to the planner, for at most three rounds |
| [`3-review-gate`](3-review-gate) | a feature over three GitHub repositories waits on an `await` step for the review of its pull requests: a bridge turns GitHub into its outcome: a comment goes to a reviser of its own, the merge of the root's accepts it, a close cancels and cleans up |
| [`4-context-budget`](4-context-budget) | an agent on claude and one on codex read more than the window of the addon `context-budget` allows: each session compacts, and the addon keeps the count as a label on the bead of its run |

A demo is `pack/` (agents and formulas, not a word about the task), `root/` (the orchestrator config of the demo,
the project and its tracker) and the repositories of the project, empty before a run: `run.sh` makes each a git
repository of its own (demos 1 and 2: `frontend/` and `backend/`) or a clone of its GitHub repository (demo 3), so a
coder's commit stays in it. Demo 4's project has no rigs: `run.sh` copies the documents its agents read into its root,
`library/`. `run.sh` of a demo runs it on a
daemon of its own; what it creates is in `.gitignore`, and the author's run is
next to it as `*-example`, with when and on what it ran. `cargo run -p aae-acceptance -- 20` runs the demos as rows
of the acceptance report.

## Roadmap

aae develops itself: the agent setup of this repository as an aae pack. Not in the first release.

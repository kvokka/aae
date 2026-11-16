# Demo 2: checks that send the work back

Demo 1, with a check of the result and a return: the same request, the same layout, and after the coders two checks.
What they find goes back to the planner, who files the fixes as new beads; the coders do them and the checks run
again, for at most three rounds.

The request is one sentence: "Create me snake game in browser and a simple backend which store the high-scores, KISS".

- `pack/` is the pack `dev`, with not a word about the task. Agent `planner` files the work, agent `coder` does one
  bead, agent `qa` tries the result; each is `agents/<agent>/agent.toml` and the prompt its steps get,
  `agents/<agent>/prompt.template.md`; its `scope` is `root` (planner and qa, over all the rigs) or `rig` (the coder,
  in its bead's rig). Formula `deliver` (`rounds = 3`):
  1. `plan`, an ai step of `planner`: it files beads under the request, each with its task, the prefix of its rig
     and the label `agent:`; from the second round on its prompt has the remarks of the checks, and it files only the fixes;
  2. `work`, a `drain` step: every ready bead under the request gets a run of `implement`, two at once (`pool = 2`):
     two rigs work at the same time;
  3. `format`, an exec check: `rumdl check --fix` over the Markdown of every rig fixes what it can; what is left
     fails the check, with rumdl's output as the remark;
  4. `qa`, a prompt check of `qa`: it starts what was built and tries the conditions of the project; what does not
     hold is its remark.

  Both checks have `back_to = "plan"`: when either fails, the run goes back to `plan` with the remarks of both. A
  check still failing in the third round fails the run.
- `root/` is the root repository: `aae.toml` (the project, its rigs and what the agents run on),
  `.aae/agents/qa/checks.md` (the conditions `qa` checks: the project's, not the pack's; the template of `qa` takes
  them as `{{ agent.checks }}`) and its tracker, made by the run. `root/config.toml` is the orchestrator config of the demo: `run.sh` starts a daemon of the demo's own with it,
  so the demo neither reads nor changes your `~/.config/aae`.
- `frontend/` and `backend/` are the two repositories of the project, empty before a run.

Every agent runs on codex, on the openai models: `planner` on `sol`, `coder` on `luna`, `qa` on `terra`.

## Run

You need `aae`, a login of `codex`, `br`, `jq`, `rumdl` and `agent-browser` with a browser: `agent-browser install`
fetches Chrome once; where Chrome has no build (Linux on arm64), set `executablePath` of a Chromium in
`~/.agent-browser/config.json`. `mise install` in this directory installs all but `aae` and the browser (mise is a
convenience, any install does). The run spends real tokens.

```sh
aae config check --config root/config.toml   # the pack and the project load
./run.sh                                     # or: mise run demo
```

`run.sh` takes some minutes and writes `RESULT.md`: when and on what it ran, how many rounds of the checks it took,
the bead tree, the runs and the outcome. Everything a run creates is in `.gitignore`; `git clean -ffdX -- .` clears
it for the next run. Compare with the author's run: `frontend-example/`, `backend-example/` and
`RESULT-example.md`, with its date, versions and models. Your result differs: the agents decide anew each time.

The author's run (2026-11-15, `RESULT-example.md`) needed two rounds: the first round of the checks sent the work
back to the planner, which added a bead to hide the game overlay during play; in the second round `qa` played the game
in the browser and saw a score reach the backend. How a return goes is in `crates/aae-testkit/tests/core/returns.rs`,
without a model.

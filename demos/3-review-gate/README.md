# Demo 3: a feature gated on the review of its pull requests

A feature over three GitHub repositories, a root and its two children, that waits for its review between the steps:
the planner and the coders do the work, a script opens a pull request in each repository, and the run stops on an
`await` step until the review decides. The bridge, a script next to the daemon, gives the review's outcome from
GitHub: a comment of an allowed author sends the work back to an agent of its own, the reviser; the merge of the
root's pull request accepts it, and the formula merges the children's; a close without a merge cancels the feature.

The request is one sentence: "Let a visitor of the site type their name and see a greeting the backend makes for it,
KISS".

- `pack/` is the pack `dev`, with not a word about the task:
  - agents `planner` (files the beads), `coder` (does one bead in its rig and commits it) and `reviser` (works the
    comments of the review). The reviser reads them only from the file the formula gives it: a `PreToolUse` hook of
    its own, in its `agent.toml`, denies it every shell command that calls `gh`;
  - formula `feature-base` (`internal = true`): `plan`, an ai step of `planner`, and `work`, a `drain` step that gives
    every ready bead a run of `implement`, two at once;
  - formula `feature`, the one a user starts:
    1. `build`, a `formula` step that runs `feature-base`;
    2. `collect`, an exec step with `if = { back_from = "review" }`: after a return from the review, it takes the
       comments and reviews of the authors in the input `feedback_from` (logins, comma-separated; none given,
       nobody's) that it has not taken yet, writes them to `root/feedback/<bead>.md` and marks each with the reaction
       "eyes";
    3. `address`, an ai step of `reviser` with the same `if`;
    4. `pr`, an exec step: in each repository with work of the feature it moves that work to the branch
       `feature/<bead>`, pushes it and opens or updates the pull request; the root's comes last, with a page that
       links the others, and its address is the last line of the step's output;
    5. `review`, an `await` step with `back_to = "collect"`: it waits for `loop-manager run resolve <run> review pass|fail`,
       which the bridge sends;
    6. `merge`, an exec step: it merges the children's pull requests still open (`max_attempts = 3`, a repeat merges
       what is left) and takes each repository back to its default branch.

    Each repository keeps the feature on one branch over every round, so `feature` has no `worktree`, which is a branch
    per run. Every script can run again with the same result;
  - formula `cleanup`: closes the open pull requests of a feature, deletes its branches, takes each repository back to
    its default branch as the remote has it and closes its bead.
- `root/` holds the configuration and the tracker of the project; its repository, `[root] path`, is `gate-demo/`.
  `loop-manager.toml` sets the models, `feedback_from` (`kvokka-agent`; put your login there) and two triggers: `cleanup` on
  `run.cancelled` of a `feature` run, and on nothing else (a failed run is left for a human to look into), and
  `owner-review` on `run.awaiting` of a `feature` run. `owner-review` is the project's own formula: its agent `owner`,
  in `root/agents/owner`, stands in for a human reviewer and acts only through `gh`; the first time it leaves one
  comment that asks for a change, the next time it merges the root's pull request. As the author of the pull requests
  it can neither approve them nor request changes: GitHub does not let one account do that on its own. It runs as the
  user does (`inherit_host = true`), and its `gh` takes the token of the host's login (`GITHUB_TOKEN`, below).
  `root/config.toml` is the orchestrator config of the demo; its `[exec]` is what the steps get, below.
- `bridge.sh` is the bridge, below.
- `gate-demo/`, `gate-demo-backend/` and `gate-demo-frontend/` are the clones of the GitHub repositories of the same
  names, made by `run.sh`; empty before a run.

The agents run on codex: `planner` and `owner` on `terra`, `coder` and `reviser` on `luna`.

## The bridge

`bridge.sh <project>` is a script that `run.sh` starts next to the daemon; it is not a `[[service]]`, which lives only
while there are sessions and gets no socket of the API. It follows the daemon's events (`loop-manager events`): a
`run.awaiting` wakes it, and it reads the runs that wait with `loop-manager run list --awaiting`, also once every interval,
which covers a run that waited before it started. While a run of the project waits on `review`, it looks at the
feature's pull requests through `gh` every `INTERVAL` seconds (30 by default); with no run waiting, it asks GitHub
nothing. The root's pull request is the last line of the output of `pr`, the others are those of the branch
`feature/<bead>` in each repository.

- The root's pull request merged: `loop-manager run resolve <run> review pass`, with `{"merged_by": <login>}` as the data.
- Closed without a merge: `loop-manager run cancel <run>` with the reason `closed without a merge: <address>`; the trigger
  `cleanup-cancelled` then clears the feature.
- A comment or a review on any of the feature's pull requests, by an author in the run's `feedback_from`, that the
  account of `gh` has not marked with the reaction "eyes": `loop-manager run resolve <run> review fail` with
  `review: <the comment's address>` as the summary. `collect` marks each comment it takes, so a comment sends the
  feature back once. The comments of anyone else, and every comment when `feedback_from` is empty, change nothing.

Each outcome goes with the idempotency key `bridge-<run>-<round>`: a repeat of it changes nothing, and it cannot
reach the next round's `review`. `crates/loop-manager/tests/e2e/review_gate.rs` runs the pack, the scripts and the bridge
without a model and without GitHub: the real `codex` on fake model APIs, a fake `gh` over local remotes.

## Run

You need `loop-manager`, a login of `codex`, `gh` logged in as an account that may push to the repositories (or its token in
`GITHUB_TOKEN`), `br` and `jq`.
`mise install` in this directory installs all but `loop-manager` (mise is a convenience, any install does). The repositories
`gate-demo`, `gate-demo-backend` and `gate-demo-frontend` must exist under the GitHub account `OWNER`
(`kvokka-agent` by default), each with a default branch. The run spends real tokens and opens real pull requests.

```sh
loop-manager config check --config root/config.toml   # the pack and the project load
OWNER=<your login> ./run.sh                  # or: mise run demo
```

`run.sh` starts the daemon and the bridge, starts the feature and waits for it: the reviewer `owner` comments once,
the bridge sends the feature back, the reviser works the comment, `owner` merges the root's pull request and the
bridge passes the review. Then it writes `RESULT.md`: when and on what it ran, the rounds of the review, the root's
pull request, the runs and the outcome. Everything a run creates is in `.gitignore`; `git clean -ffdX -- .` clears it
for the next run. An exec step gets a clean environment (`PATH`, `HOME`, `LANG`), which a login of `gh` in a keyring
does not reach: `run.sh` puts the token of the host's `gh` into `GITHUB_TOKEN`, unless it is set already, and the
steps and `owner` name it by reference, `GITHUB_TOKEN = "${GITHUB_TOKEN}"`, in `[exec] env` of `root/config.toml` and
in the `env` of `owner`. The same `[exec] env` gives the steps a literal, `MY_AWESOME_ENV_VAR`, which the step `merge`
prints. Keys, layers and references: `docs/design/config.md`, "Environment values". Compare with the
author's run: `gate-demo-example/`, `gate-demo-backend-example/`, `gate-demo-frontend-example/` and
`RESULT-example.md`, with its date, versions and models. Your result differs: the agents decide anew each time.

The author's run (2026-11-17, in `RESULT-example.md`) passed in two rounds of the review, as the demo means it to:
`owner` asked the backend to answer a name that is not valid UTF-8 with a JSON 400 instead of failing the request,
the reviser did, and `owner` merged the root's pull request,
[gate-demo#3](https://github.com/kvokka-agent/gate-demo/pull/3); the feature merged
[gate-demo-backend#3](https://github.com/kvokka-agent/gate-demo-backend/pull/3) and
[gate-demo-frontend#3](https://github.com/kvokka-agent/gate-demo-frontend/pull/3), and the step `merge` printed
`MY_AWESOME_ENV_VAR is something`. The pull requests #1 and #2 are of runs before it.

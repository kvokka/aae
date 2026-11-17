# Acceptance scenarios

`crates/aae-acceptance` holds end-to-end scenarios of aae, one per criterion below, each on a real `aae daemon` in a
temporary directory tree and on the real harness binaries. They go the way working runs go: the product has no
separate test mode.

They are not part of the everyday checks, and they are allowed to be slow. `mise run ci` is the gate of a change
([development](guide/development.md)); the scenarios run when the owner of the repository asks for them, before a
release or to check one area after a change in it. Nothing starts them on a commit or on a schedule.

The model-free rows run in CI, in the workflow `acceptance`, which starts by hand only
(`gh workflow run acceptance`). The rows on a real model and the demos cannot run there, since CI has no access to a
model: they run on a developer's machine, on the same request.

| Command | What runs | Needs |
|---|---|---|
| `cargo build -p aae && cargo run -p aae-acceptance -- --ci 3 19` | the named criteria, model-free rows only | nothing |
| `mise run acceptance-ci` | every model-free row: a fake provider, the scripted driver, exec steps, the chaos stand; what the workflow `acceptance` runs | nothing |
| `mise run acceptance` | every row on the release build, the live-model rows and the live runs of demos 1 and 2 among them | a harness login; it spends real tokens |
| `mise run acceptance-release` | criterion 1 with the four release targets; the workflow `release` runs it on a `v*` tag | zig, cargo-zigbuild |

A run writes one report, `target/acceptance/report.md`: a line for every row, run or not, with the date, the version
of `aae` and the versions of the harnesses. A row that is not run cannot fail, so a green model-free run says nothing
of the live-model rows. After the scenarios the same binary runs the rows that live in cargo and the scripts, one at
a time: the release build (1), the crash tests of the adapters (9), the contract tests (11), the line budgets (22),
the stub linters (23) and the chaos stand (35). The crash scenario (8) prints its pause seed; `AAE_CRASH_SEED=<n>`
replays it, and the chaos stand does the same with `AAE_CHAOS_SEED` ([development](guide/development.md#the-chaos-stand)).

The live-model rows use the cheapest model of each harness, set by the agents of their fixtures. The checks run on
Linux arm64; the other three targets are built (criterion 1) and not exercised. The numeric thresholds were proposed
by the author of the criteria and may be changed on evidence. `default-policy` is on in every scenario unless a row
says otherwise.

## Criteria

`model-free` rows need no login. `live` rows call a real model. There is no number 21.

| № | Criterion | Kind |
|---|---|---|
| 1 | One binary builds for the four targets; the Linux arm64 build runs on the host without extra runtimes | build |
| 2 | A formula of one `ai` step passes as a real run on a cheap model through claude and through codex, ended `pass` by the finisher | live |
| 3 | A formula of one `exec` step passes; exit 75 fails the attempt, not the formula | model-free |
| 4 | A turn that ends without the finisher gives no `pass`; the finisher of a stale attempt is refused; a hook on `run.finishing` returns a remark and the run passes on the second go in the same attempt | model-free |
| 4a | A finisher called while a background claude subagent runs does not end the run before the session is idle without background | live |
| 5a | A chain without a model: an `exec` planner formula files three beads of an epic; a subscription to `bead.ready` starts an executor on "the next ready"; all three beads are closed, all runs share one correlation | model-free |
| 5b | A schedule fires on the managed clock; after the daemon was down a missed firing fires exactly once | model-free |
| 6 | The hop from the commit of `run.passed` of run A to the launch of the harness process of run B, started by a subscription, with no bead, no worktree and no veto hooks: over 30 hops p50 ≤ 1 s, p90 ≤ 3 s | model-free |
| 7 | The idle daemon with one registered project: RSS ≤ 50 MB; CPU ≤ 1 % of one core on average over 30 seconds | model-free |
| 8 | `kill -9` of the daemon mid-turn, 20 times. After each: the daemon takes commands within 30 s; every run reaches a terminal state exactly once; no trigger firing is lost or doubled; no orphan processes | model-free on the scripted driver; 5 repeats on the real harnesses are live |
| 9 | `kill -9` of the harness mid-tool: the failure is typed within 1 s, the run is resumed and reaches its outcome | model-free |
| 10 | A codex quota with a reset time on a fake provider: the account gate closes, a second run of the account does not start, after the reset both pass without a human. The claude half is not verified: the reset time comes only with a subscription login, and burning quota for a check is forbidden | model-free |
| 11 | The contract tests of claude and codex pass ([harness updates](harness-updates.md)); the tracker contract tests pass for br ([tracker](design/tracker-br.md)) | model-free |
| 12 | "Explain config" shows the layer of every value, the host layer as one line. With a stand-in home directory of the harness the same agent works with host inheritance and without it, and the session shows the difference | model-free |
| 13 | Two agents with different process parts of the codex config get two app-servers, with the same part one; an instance starts lazily and stops after idling | model-free |
| 14 | One HTTP MCP service serves concurrent claude and codex sessions with identity headers; the number of its processes does not grow with the sessions | model-free |
| 15 | An external example plugin gives a skill and a harness hook to one of two agents only, and vetoes a `pass`; a crash of its hook does not bring the daemon down | model-free |
| 16 | A run with a bead: the daemon takes the claim and holds it through the death of a session and its resume; `failed` returns the bead, `passed` closes it; "the next ready in the epic" under 5 concurrent requests gives each bead to exactly one run; after the set number of failures the bead is given out no more; every session has the database path; br holds no bookkeeping records | model-free |
| 17 | Every operation of the HTTP API has a CLI command; the OpenAPI document matches the code; MCP has five tools | model-free; one row with a real claude session is live |
| 18 | The journal rotates at 10 MB × 5; a reader continues by `seq`; a `seq` rotated away gives an explicit gap | model-free |
| 19 | Gates closed by the operator (orchestrator, project, agent, bead) stop dispatch, survive a daemon restart and are not opened by automation | model-free |
| 20 | Demos 1 and 2 ([demos](../demos/README.md)) go from the user's request to the result unattended: in demo 1 every bead under the root bead is closed, in demo 2 the checks are green by the third round at the latest. Demo 2 passes two runs in a row. The pack and `root/config.toml` of every demo, demo 3 too, load (`aae config check` is clean); demo 3's pack, step scripts and bridge also run without a model and without GitHub in `mise run ci` (`crates/aae/tests/e2e/review_gate.rs`) | live; the loading and the packs on fake providers are model-free |
| 22 | Non-test, non-generated code is at most 30 000 lines, the core at most 18 000; every `MEASURE` marker has its required fields | model-free |
| 23 | No stubs and no code "for later": no `todo!` or `unimplemented!`, no unused public items, no states or operations for what the first release leaves out | model-free |
| 24 | A formula of several steps without a model: the first step files beads under the root bead with rig prefixes and agent and harness labels; `drain` gives each its own run with the agent, rig and harness of the bead; one failed bead does not hinder the rest; the last step sees which beads are closed. `kill -9` of the daemon in every step: the run goes on from the same step, no step and no bead is executed twice | model-free |
| 25 | A return: a check step fails, the formula goes back to the named step with its remarks and passes from the next round; with a lasting failure the run ends `failed` after exactly the set number of rounds, the remarks in its result | model-free |
| 26 | A pack from a git repository: by tag, by sha and unpinned. `aae` downloads it; loading the config again with a tag or sha that is in the cache touches no network; the CLI command clears the cache, after which the pack is downloaded again | model-free |
| 27 | The user's config: an orchestrator without a `[harness.<name>]` section runs an agent on a harness of the build; `[agents.<name>]` edits a pack's agent and declares a variant, and "explain config" names the layer; `permission_mode` in the common settings and an `[overrides.*]` section are config errors that say where it is written now; the root takes a name and a path, `root` next to `aae.toml` without them; a root that is a directory without git does not hinder the work | model-free |
| 28 | A task: starting a formula with a text files a root bead, starting it with the id of an existing bead takes that bead up and files no second one; a formula declares no input of type `bead` | model-free |
| 29 | A repeat of a step: a step with a check script that fails twice and passes the third time gives `pass` with `max_attempts = 3` and `failed` with `max_attempts = 2`; exit 75 of the script fails the step with no repeat; a repeat of an `ai` step gets the script's output as a remark in the same session. `kill -9` of the daemon between attempts does not reset the count | model-free |
| 30 | `drain`: with `pool = 2` and five ready beads at most two are in work at once, without `pool` one; `on_item_failure`: `continue` finishes the other beads, `soft_stop` ends the started ones and gives no new, `hard_stop` cancels the started; after a stop the step is `fail` | model-free |
| 31 | A `formula` step: the nested formula runs as a child run, gets the inherited inputs only of those it declares and those set on the step, and its outcome is the step's. A formula with `internal = true` does not start directly, is hidden in the catalogue, shown with `--all` and marked in the API. Formulas of two projects with one name do not see each other | model-free |
| 32 | Scope and prefixes: a step of an agent of scope `root` works in the root, of scope `rig` in the rig; the beads of rigs carry the rig's prefix (the acronym of the name, or the `prefix` key), and the rig of a child run of `drain` is taken from the prefix; in a project of one repository without rigs an agent of scope `rig` does the root's bead in the root | model-free |
| 33 | An agent from the project: `prompt.template.md` from `.aae/agents/<agent>/` of the root replaces the pack's template; a file of the project (`checks.md`) is substituted into the agent's template; a missing template or named file is a config error that names the path | model-free |
| 34 | Background after the outcome: an agent declared its outcome and left a background command; the step ends without waiting out the background budget and no orphan process stays; without a declared outcome the background is waited for as before | model-free |
| 35 | The chaos stand: on 1000 new seeds a random sequence of commands, harness behaviour, hooks, operator actions, repeated and lost deliveries and `kill -9` of the daemon keeps every [invariant](design/invariants.md) of the core after every step, and every run reaches an end once the faults stop | model-free |

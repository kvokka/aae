# aae acceptance spec

- date: 2026-11-16
- aae: aae 0.1.0 9e250c2-dirty
- claude: 2.1.325 (Claude Code)
- codex: codex-cli 0.168.0
- br: br 0.7.6
- host: Linux aarch64
- run: CI subset

<!-- MEASURE id=acceptance-times date=2026-11-16 ver=aae-0.1.0-9e250c2-dirty env=Linux-aarch64 src=aae-acceptance note="wall time of each row; the scenarios run concurrently, then the checks one by one" -->
| criterion | check | result | time | notes |
|---|---|---|---|---|
| 1 | one binary builds for the four targets; the build for this host runs without extra runtimes | not run | 0.0 s | release builds: run on a tag, or the spec without --ci |
| 2 | an ai step passes on the cheapest real claude model by the finisher | not run | 0.0 s | needs a real model: run the spec without --ci |
| 2 | an ai step passes on the cheapest real codex model by the finisher | not run | 0.0 s | needs a real model: run the spec without --ci |
| 3 | an exec step passes; exit 75 fails the attempt, not the formula | pass | 1.3 s |  |
| 4 | no pass without the finisher; a stale finisher is rejected; a remark passes on the second go | pass | 1.2 s |  |
| 4a | a finisher called with a background claude subagent running passes the run only after the subagent | not run | 0.0 s | needs a real model: run the spec without --ci |
| 5a | an exec planner creates three beads of an epic; a subscription closes each; one correlation | pass | 1.9 s | 4 runs in one correlation "cor_1" |
| 5b | a schedule fires on time; the firings missed while the daemon is down fire once | pass | 13.2 s | every 2 s; 2 firings, 3 missed while down, 1 at start (0 ms after it), the next 2003 ms later |
| 5b | a cron and an hourly schedule fire on the managed clock, and once after downtime | pass | 5.2 s | Summary [   0.399s] 1 test run: 1 passed, 211 skipped |
| 6 | 30 hops from run.passed of A to the harness launch of B by a subscription: p50 <= 1 s, p90 <= 3 s | pass | 10.1 s | 30 hops: p50 7 ms, p90 9 ms, max 27 ms <!-- MEASURE id=hop date=2026-11-16 ver=aae-0.1.0-9e250c2-dirty env=Linux-aarch64 src=aae-acceptance note="run.passed ts of A to instance.ready ts of B; claude on a fake API" --> |
| 7 | the idle daemon with one project: RSS <= 50 MB, CPU <= 1 % over 30 seconds | pass | 35.3 s | RSS max 32.8 MB, CPU 0.033 % over 30 s (2026-11-16, aae 0.1.0 9e250c2-dirty) <!-- MEASURE id=idle-daemon date=2026-11-16 ver=aae-0.1.0-9e250c2-dirty env=Linux-aarch64 src=aae-acceptance note="one project, no runs; RSS sampled every 1 s, CPU from /proc stat" --> |
| 8 | 20 x kill -9 of the daemon mid-step: up within 30 s, each run ends once, no firing lost or doubled | pass | 11.2 s | 20 kills (pause seed 10472916328057878351), 40 runs, 20 attempts failed by the crashes; slowest restart to the first answer 0.03 s |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real claude model: each run resumes and ends once, a passed one fires once | not run | 0.0 s | needs a real model: run the spec without --ci |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real codex model: each run resumes and ends once, a passed one fires once | not run | 0.0 s | needs a real model: run the spec without --ci |
| 9 | kill -9 of claude and of the codex app-server mid-tool is typed, resumed and reaches its outcome | pass | 8.6 s | Summary [   2.404s] 2 tests run: 2 passed, 63 skipped |
| 10 | a codex quota with a reset time closes the account gate; both runs of the account pass after it | pass | 11.3 s | gate "account:codex/3759e2213c172fb1" until "2026-11-16T05:22:06.000Z"; opened 10059 ms after the limit, nothing of the second run before |
| 11 | the contract tests of claude and codex (§7.8), codex's model list, and of the br tracker (§10.2) pass | pass | 23.8 s | Summary [  19.837s] 40 tests run: 40 passed, 0 skipped |
| 12 | explain config shows every layer and the host in one line; host inheritance shows in the session | pass | 1.2 s | explain: 10 and 10 lines, one host line each; the host CLAUDE.md reached the agent only while it inherited |
| 13 | codex agents of one process part share an app-server, of two get two; lazy start, idle stop | pass | 4.0 s | 3 runs on 2 app-servers (i-0e3d2cb7278b8a92, i-51944e7c4ae6cbff); both stopped as idle |
| 14 | one HTTP MCP service serves concurrent claude and codex sessions with identity headers in one process | pass | 1.9 s | 2 claude and 2 codex runs passed on one service process (pid 3303279); 14 requests, all with the token and each run's identity headers; one stdio MCP server behind the aae bridge (pid 3303305) served every run once each, initialized once, the run in each call's _meta |
| 15 | an external plugin gives its skill and hook to one of two agents, vetoes a pass; its failure is contained | pass | 1.6 s | veto: the summary of a pass must say the work is tested; skill, hook, prompt fragment and bin/ only in the attaching agent's session; a failed hook: the review plugin needs its require parameter |
| 16 | a bead: claim held through a crash, returned on failure, closed on pass, one run per bead | pass | 6.3 s | claim held through a crash; 5 requests for an epic of 3: 3 runs, 2 nothing_ready; out after 2 failures |
| 16 | the claim holds through the death of a claude session and its resume; the session has the tracker's path | pass | 2.2 s | Summary [   2.119s] 1 test run: 1 passed, 64 skipped |
| 17 | a CLI command per API operation, the committed OpenAPI matches the code and real replies fit it, MCP serves its five tools | pass | 1.2 s | Summary [   0.287s] 7 tests run: 7 passed, 64 skipped |
| 17 | a session of the cheapest real claude model calls list_formulas on the daemon's MCP and reports it | not run | 0.0 s | needs a real model: run the spec without --ci |
| 18 | the journal rotates at 10 MiB x 5; a reader continues by seq; a rotated-away seq is a gap | pass | 0.6 s | rotated after 6 runs |
| 19 | operator gates stop dispatch, survive a restart and stay until opened by hand | pass | 1.3 s |  |
| 20 | demo 2's format step on a rig its repository ignores: rumdl fixes what it can; the rest fails the step with rumdl's findings | pass | 0.4 s | fixed a rig the repository ignores and passed; an unfixable file failed the step with rumdl's findings |
| 20 | demo 2's pack in a second round: format fails, the planner gets rumdl's findings and files a fix, the drain runs only the new bead, format and qa pass | pass | 2.9 s | round 1: f-vsb failed format with MD052, back to plan; round 2: the planner saw the remarks, the drain ran only f-rig, format and qa passed |
| 20 | the pack and the project of each demo in demos/ load | pass | 0.0 s | `aae config check` of demos/<demo>/root/config.toml is clean for each demo |
| 20 | demo 1: a request, a planner that files beads, a drain to coders on claude and codex, a report that every bead under the root is closed, unattended | not run | 0.0 s | needs a real model: run the spec without --ci |
| 20 | demo 2: demo 1 on openai models, then a rumdl check and a qa agent in a browser send the work back to the planner until both pass, in at most three rounds, unattended | not run | 0.0 s | needs a real model: run the spec without --ci |
| 20 | the demos of the current config shape (runs without a rig, agent scope, the project root) have not run live | open | 0.0 s | no row checks this part yet: the criterion stays open |
| 22 | line budgets, MEASURE markers, a current readiness file whose scores the rows allow, all 8 or more | pass | 0.3 s | budget: total      16938 / 30000 (hard stop 35000); readiness: below 8: Event loop and state 6, Runs and finisher 6, Recovery 6, Config assembly 6, claude adapter 6, codex adapter 6, br adapter 6, `default-policy` 6, Acceptance spec 6, Delivery and `doctor` 6 |
| 23 | no todo!/unimplemented!, no unused public items or dependencies (the rest is review) | pass | 1.4 s | clippy (todo, unimplemented, unreachable_pub denied), cargo machete and scripts/unused-pub.sh are clean |
| 24 | a request's root gets beads with rig prefixes and agent and harness labels; drain gives each a run; kill -9 in every step: no step and no bead runs twice | pass | 3.3 s | ship: plan, drain, report, each through a kill -9; w-92s, a-gxx closed, a-whl under t-v5n.1 failed once, w-654 refused |
| 25 | a failing check sends the run back with its remarks; past the formula's rounds the run fails | pass | 0.3 s | passed in round 2; failed after exactly 2 rounds with the remarks |
| 26 | a pack from git by tag, by sha and unpinned: aae downloads it; a cached tag or sha needs no network; `cache clear` makes it download again | pass | 1.0 s | a tag, a commit and the latest revision downloaded by the daemon; the cache serves offline; clear downloads again |
| 27 | a harness of the build needs no section; `[agents.<name>]` edits an agent or declares a variant and explain names its layer; a retired key is one line that says where it went; the root takes a name and a path, root and next to aae.toml without them, need not be a git repository, and runs the project triggers without a rig | pass | 2.3 s | Summary [   0.070s] 12 tests run: 12 passed, 316 skipped |
| 28 | a task: text files the root bead, an id of a bead takes it up, no second bead | open | 0.0 s | no row checks this part yet: the criterion stays open |
| 29 | a step repeats up to `max_attempts` while its check script fails; exit code 75 fails it at once | open | 0.0 s | no row checks this part yet: the criterion stays open |
| 30 | `drain` keeps up to `pool` items in work and follows `on_item_failure` | open | 0.0 s | no row checks this part yet: the criterion stays open |
| 31 | a `formula` step runs its formula as a child run; formulas with `internal = true` | open | 0.0 s | no row checks this part yet: the criterion stays open |
| 32 | an ai step of an agent of scope root works in the root, of scope rig in the rig, and no rig edits it; a project of one repository runs without rigs, its beads in the root; a rig's beads carry its prefix, the acronym of its name or its `prefix` key, and `drain` takes the rig of a child from it | pass | 4.7 s | Summary [   0.208s] 12 tests run: 12 passed, 323 skipped |
| 33 | an agent's prompt template and files from the project | open | 0.0 s | no row checks this part yet: the criterion stays open |
| 34 | a step ends once its outcome is declared, without waiting for the background, and leaves no orphan | open | 0.0 s | no row checks this part yet: the criterion stays open |

30 of 46 rows passed, 0 failed, 9 not run, 7 open, in 85 s.

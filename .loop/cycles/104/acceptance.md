# aae acceptance spec

- date: 2026-11-16
- aae: aae 0.1.0 6da1e31-dirty
- claude: 2.1.325 (Claude Code)
- codex: codex-cli 0.168.0
- br: br 0.7.6
- host: Linux aarch64
- run: CI subset

<!-- MEASURE id=acceptance-times date=2026-11-16 ver=aae-0.1.0-6da1e31-dirty env=Linux-aarch64 src=aae-acceptance note="wall time of each row; the scenarios run concurrently, then the checks one by one" -->
| criterion | check | result | time | notes |
|---|---|---|---|---|
| 1 | the release build for this host's target runs without extra runtimes | not run | 0.0 s | a request row: run the spec without --ci |
| 1 | one binary builds for each of the four targets | on a tag | 0.0 s | built on a tag: `mise run acceptance-release` |
| 2 | an ai step passes on the cheapest real claude model by the finisher | not run | 0.0 s | a request row: run the spec without --ci |
| 2 | an ai step passes on the cheapest real codex model by the finisher | not run | 0.0 s | a request row: run the spec without --ci |
| 3 | an exec step passes; exit 75 fails the attempt, not the formula | pass | 1.2 s |  |
| 4 | no pass without the finisher; a stale finisher is rejected; a remark passes on the second go | pass | 1.8 s |  |
| 4a | a finisher called with a background claude subagent running passes the run only after the subagent | not run | 0.0 s | a request row: run the spec without --ci |
| 5a | an exec planner creates three beads of an epic; a subscription closes each; one correlation | pass | 2.8 s | 4 runs in one correlation "cor_1" |
| 5b | a schedule fires on time; the firings missed while the daemon is down fire once | pass | 13.1 s | every 2 s; 2 firings, 3 missed while down, 1 at start (0 ms after it), the next 2003 ms later |
| 5b | a cron and an hourly schedule fire on the managed clock, and once after downtime | pass | 0.2 s | Summary [   0.079s] 1 test run: 1 passed, 256 skipped |
| 6 | 30 hops from run.passed of A to the harness launch of B by a subscription: p50 <= 1 s, p90 <= 3 s | pass | 13.2 s | 30 hops: p50 9 ms, p90 42 ms, max 136 ms <!-- MEASURE id=hop date=2026-11-16 ver=aae-0.1.0-6da1e31-dirty env=Linux-aarch64 src=aae-acceptance note="run.passed ts of A to instance.ready ts of B; claude on a fake API" --> |
| 7 | the idle daemon with one project: RSS <= 50 MB, CPU <= 1 % over 30 seconds | pass | 35.2 s | RSS max 32.4 MB, CPU 0.067 % over 30 s (2026-11-16, aae 0.1.0 6da1e31-dirty) <!-- MEASURE id=idle-daemon date=2026-11-16 ver=aae-0.1.0-6da1e31-dirty env=Linux-aarch64 src=aae-acceptance note="one project, no runs; RSS sampled every 1 s, CPU from /proc stat" --> |
| 8 | 20 x kill -9 of the daemon mid-step: up within 30 s, each run ends once, no firing lost or doubled | pass | 13.4 s | 20 kills (pause seed 16727109011044523472), 40 runs, 20 attempts failed by the crashes; slowest restart to the first answer 0.05 s |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real claude model: each run resumes and ends once, a passed one fires once | not run | 0.0 s | a request row: run the spec without --ci |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real codex model: each run resumes and ends once, a passed one fires once | not run | 0.0 s | a request row: run the spec without --ci |
| 9 | kill -9 of claude and of the codex app-server mid-tool is typed, resumed and reaches its outcome | pass | 2.6 s | Summary [   2.286s] 2 tests run: 2 passed, 64 skipped |
| 10 | a codex quota with a reset time closes the account gate; both runs of the account pass after it | pass | 12.1 s | gate "account:codex/935c95d918514b9b" until "2026-11-16T15:08:32.000Z"; opened 10408 ms after the limit, nothing of the second run before |
| 11 | the contract tests of claude and codex (§7.8), codex's model list, and of the br tracker (§10.2) pass | pass | 19.4 s | Summary [  19.258s] 41 tests run: 41 passed, 0 skipped |
| 12 | explain config shows every layer and the host in one line; host inheritance shows in the session | pass | 1.7 s | explain: 10 and 10 lines, one host line each; the host CLAUDE.md reached the agent only while it inherited |
| 13 | codex agents of one process part share an app-server, of two get two; lazy start, idle stop | pass | 4.6 s | 3 runs on 2 app-servers (i-89b2177f6c8ea62d, i-24ff1ed5a9cba1c9); both stopped as idle |
| 14 | one HTTP MCP service serves concurrent claude and codex sessions with identity headers in one process | pass | 2.7 s | 2 claude and 2 codex runs passed on one service process (pid 1111609); 14 requests, all with the token and each run's identity headers; one stdio MCP server behind the aae bridge (pid 1111620) served every run once each, initialized once, the run in each call's _meta |
| 15 | an external plugin gives its skill and hook to one of two agents, vetoes a pass; its failure is contained | pass | 2.3 s | veto: the summary of a pass must say the work is tested; skill, hook, prompt fragment and bin/ only in the attaching agent's session; a failed hook: the review plugin needs its require parameter |
| 16 | a bead: claim held through a crash, returned on failure, closed on pass, one run per bead | pass | 7.7 s | claim held through a crash; 5 requests for an epic of 3: 3 runs, 2 nothing_ready; out after 2 failures |
| 16 | the claim holds through the death of a claude session and its resume; the session has the tracker's path | pass | 2.3 s | Summary [   2.186s] 1 test run: 1 passed, 65 skipped |
| 17 | a CLI command per API operation, the committed OpenAPI matches the code, its path parameters are the names in its paths, real replies fit it, MCP serves its five tools | pass | 0.5 s | Summary [   0.324s] 8 tests run: 8 passed, 65 skipped |
| 17 | a session of the cheapest real claude model calls list_formulas on the daemon's MCP and reports it | not run | 0.0 s | a request row: run the spec without --ci |
| 18 | the journal rotates at 10 MiB x 5; a reader continues by seq; a rotated-away seq is a gap | pass | 0.8 s | rotated after 6 runs |
| 19 | operator gates stop dispatch, survive a restart and stay until opened by hand | pass | 2.0 s |  |
| 20 | demo 2's format step on a rig its repository ignores: rumdl fixes what it can; the rest fails the step with rumdl's findings | pass | 0.4 s | fixed a rig the repository ignores and passed; an unfixable file failed the step with rumdl's findings |
| 20 | demo 1's pack: beads with rig prefixes, coders on claude and codex, the report finds every bead closed | pass | 3.1 s | f-bjr on claude and api-94c on codex, filed by the planner with the rig prefixes, closed; report passed |
| 20 | demo 2's pack in a second round: format fails, the planner gets rumdl's findings and files a fix, the drain runs only the new bead, format and qa pass | pass | 3.7 s | round 1: f-bu4 failed format with MD052, back to plan; round 2: the planner saw the remarks, the drain ran only f-1e3, format and qa passed |
| 20 | the pack and the project of each demo in demos/ load | pass | 0.0 s | `aae config check` of demos/<demo>/root/config.toml is clean for each demo |
| 20 | demo 1: a request, a planner that files beads, a drain to coders on claude and codex, a report that every bead under the root is closed, unattended | not run | 0.0 s | a request row: run the spec without --ci |
| 20 | demo 2: demo 1 on openai models, then a rumdl check and a qa agent in a browser send the work back to the planner until both pass, in at most three rounds, unattended, two runs in a row | not run | 0.0 s | a request row: run the spec without --ci |
| 22 | line budgets, MEASURE markers, a current readiness file whose scores the rows allow, all 8 or more | FAIL | 0.5 s | docs/READINESS.md is assessed at 11839e9, not at or after 6da1e31, the last change of crates/: re-assess it |
| 23 | no todo!/unimplemented!, no unused public items or dependencies (the rest is review) | pass | 1.5 s | clippy (todo, unimplemented, unreachable_pub denied), cargo machete and scripts/unused-pub.sh are clean |
| 24 | a task's root gets beads with rig prefixes and agent and harness labels; drain gives each a run; kill -9 in every step: no step and no bead runs twice | pass | 4.3 s | ship: plan, drain, report, each through a kill -9; w-r4x, a-go7 closed, a-ja8 under t-0o1.1 failed once, w-qra refused |
| 25 | a failing check sends the run back with its remarks; past the formula's rounds the run fails | pass | 0.3 s | passed in round 2; failed after exactly 2 rounds with the remarks |
| 26 | a pack from git by tag, by sha and unpinned: aae downloads it; a cached tag or sha needs no network; `cache clear` makes it download again | pass | 1.4 s | a tag, a commit and the latest revision downloaded by the daemon; the cache serves offline; clear downloads again |
| 27 | a harness of the build needs no section; `[agents.<name>]` edits an agent or declares a variant and explain names its layer; a retired key is one line that says where it went; the root takes a name and a path, root and next to aae.toml without them, need not be a git repository, and runs the project triggers without a rig | pass | 0.2 s | Summary [   0.078s] 12 tests run: 12 passed, 371 skipped |
| 28 | a task: a text files one root bead, also across a crash, an id of a bead takes that bead up and files none, on the real br and a real daemon too; a formula declares no input of type `bead` | pass | 0.7 s | Summary [   0.555s] 7 tests run: 7 passed, 450 skipped |
| 29 | a step repeats while its check script fails, up to max_attempts, and kill -9 keeps the count; exit 75 fails it at once | pass | 0.7 s | max_attempts 3 passed on the third check across kill -9, 2 failed after two; exit 75 failed after one |
| 29 | a step runs again up to `max_attempts` while its check script fails, the script's output its remark in the same session; exit 75 fails it at once; a crash or a restart keeps the count; a round starts it again; a timeout, a signal or a lost answer of the script does not pass, a script that cannot start fails the run | pass | 1.3 s | Summary [   1.206s] 15 tests run: 15 passed, 368 skipped |
| 30 | a drain step on br keeps one bead in work, two with `pool = 2`; `continue` runs the rest after a failure, `soft_stop` ends the started across kill -9 and gives no new, `hard_stop` cancels them | pass | 11.7 s | five beads: no pool one at a time, pool 2 two at work at once, continue ran all; soft_stop ended the started across kill -9 and failed with 3 not started; hard_stop cancelled the started |
| 30 | a drain step keeps at most `pool` beads in work, 1 unless set; `on_item_failure` stops it softly or hard and fails it, a restart giving no new bead, a repeat giving them again | pass | 0.2 s | Summary [   0.049s] 7 tests run: 7 passed, 376 skipped |
| 31 | a formula step runs an internal formula as a child run with the inputs it declares and those the step gives, its outcome the step's; a direct start of it is refused, the list hides it without `--all`; one formula name in two projects runs each project's own | pass | 0.3 s | ship ran internal child as its child run: inherited goal, the step's note, not scratch; its pass and fail were ship's; a direct start was refused 409, the API marked it internal, formula list showed it only with --all; `same` of shop and of lab each ran its own, lab's `solo` was unknown to shop |
| 31 | a formula step runs its formula as a child run with the inputs it declares and the step's, the task among them on the bead the parent holds, its outcome the step's, cancelled with its parent and not doubled by a restart; an internal formula starts only from a step; the config refuses a step short of an input, one the formula lacks, a formula that runs itself and a trigger on an internal formula | pass | 0.1 s | Summary [   0.037s] 14 tests run: 14 passed, 369 skipped |
| 32 | an ai step of an agent of scope root works in the root, of scope rig in the rig, and no rig edits it; a project of one repository runs without rigs, its beads in the root; a rig's beads carry its prefix, the acronym of its name or its `prefix` key, and `drain` takes the rig of a child from it | pass | 0.4 s | Summary [   0.268s] 12 tests run: 12 passed, 379 skipped |
| 33 | an agent's prompt template is its `prompt.template.md`; the project's `.aae/agents/<agent>/` replaces it and fills it with its files; a missing template or file is a config error naming the path | pass | 0.2 s | Summary [   0.025s] 8 tests run: 8 passed, 441 skipped |
| 34 | an agent that declared its outcome and left a background command: the step ends at once and the command is stopped; without an outcome the background is waited for (claude and codex on fake providers) | pass | 21.8 s |  |

39 of 50 rows passed, 1 failed, 9 not run, 1 on a tag, 0 open, in 65 s.

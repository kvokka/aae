# aae acceptance spec

- date: 2026-11-14
- aae: aae 0.1.0
- claude: 2.1.325 (Claude Code)
- codex: codex-cli 0.168.0
- br: br 0.7.6
- host: Linux aarch64
- run: CI subset

<!-- MEASURE id=acceptance-times date=2026-11-14 ver=aae-0.1.0 env=Linux-aarch64 src=aae-acceptance note="wall time of each row; the scenarios run concurrently, then the checks one by one" -->
| criterion | check | result | time | notes |
|---|---|---|---|---|
| 1 | one binary builds for the four targets; the build for this host runs without extra runtimes | not run | 0.0 s | release builds: run on a tag, or the spec without --ci |
| 2 | a prompt step passes on the cheapest real claude model by the finisher | not run | 0.0 s | needs a real model: run the spec without --ci |
| 2 | a prompt step passes on the cheapest real codex model by the finisher | not run | 0.0 s | needs a real model: run the spec without --ci |
| 3 | an exec step passes; exit 75 fails the attempt, not the formula | pass | 1.9 s |  |
| 4 | no pass without the finisher; a stale finisher is rejected; a remark passes on the second go | pass | 1.9 s |  |
| 4a | a finisher called with a background claude subagent running passes the run only after the subagent | not run | 0.0 s | needs a real model: run the spec without --ci |
| 5a | an exec planner creates three beads of an epic; a subscription closes each; one correlation | pass | 2.5 s | 4 runs in one correlation "cor_1" |
| 5b | a schedule fires on time; the firings missed while the daemon is down fire once | pass | 14.6 s | every 2 s; 2 firings, 3 missed while down, 1 at start (0 ms after it), the next 2003 ms later |
| 6 | 30 hops from run.passed of A to the harness launch of B by a subscription: p50 <= 1 s, p90 <= 3 s | pass | 11.8 s | 30 hops: p50 6 ms, p90 15 ms, max 270 ms <!-- MEASURE id=hop date=2026-11-14 ver=aae-0.1.0 env=Linux-aarch64 src=aae-acceptance note="run.passed ts of A to instance.ready ts of B; claude on a fake API" --> |
| 7 | the idle daemon with one project: RSS <= 50 MB, CPU <= 1 % over 30 seconds | pass | 35.9 s | RSS max 32.2 MB, CPU 0.067 % over 30 s (2026-11-14, aae 0.1.0) <!-- MEASURE id=idle-daemon date=2026-11-14 ver=aae-0.1.0 env=Linux-aarch64 src=aae-acceptance note="one project, no runs; RSS sampled every 1 s, CPU from /proc stat" --> |
| 8 | 20 x kill -9 of the daemon mid-step: up within 30 s, each run ends once, no firing lost or doubled | pass | 53.6 s | 20 kills (pause seed 11339524142475530982), 40 runs, 20 attempts failed by the crashes; slowest restart to the first answer 0.05 s |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real claude model: each run resumes and ends once, a passed one fires once | not run | 0.0 s | needs a real model: run the spec without --ci |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real codex model: each run resumes and ends once, a passed one fires once | not run | 0.0 s | needs a real model: run the spec without --ci |
| 9 | kill -9 of claude and of the codex app-server mid-tool is typed, resumed and reaches its outcome | pass | 16.3 s | Summary [   2.744s] 2 tests run: 2 passed, 42 skipped |
| 10 | a codex quota with a reset time closes the account gate; both runs of the account pass after it | pass | 12.0 s | gate "account:codex/f20ee4fb6251efbc" until "2026-11-14T08:40:01.000Z"; opened 10343 ms after the limit, nothing of the second run before |
| 11 | the contract tests of claude and codex (§7.8) and of the br tracker (§10.2) pass | pass | 17.5 s | Summary [   7.486s] 35 tests run: 35 passed, 0 skipped |
| 12 | explain config shows every layer and the host in one line; host inheritance shows in the session | pass | 1.9 s | explain: 10 and 10 lines, one host line each; the host CLAUDE.md reached the agent only while it inherited |
| 13 | codex agents of one process part share an app-server, of two get two; lazy start, idle stop | pass | 4.6 s | 3 runs on 2 app-servers (i-c120c60919f3e215, i-f8400090cfedf2c7); both stopped as idle |
| 14 | one HTTP MCP service serves concurrent claude and codex sessions with identity headers in one process | pass | 2.5 s | 2 claude and 2 codex runs passed on one service process (pid 2851406); 14 requests, all with the token and each run's identity headers; one stdio MCP server behind the aae bridge (pid 2851408) served every run once each, initialized once, the run in each call's _meta |
| 15 | an external plugin gives its skill and hook to one of two agents, vetoes a pass; its failure is contained | pass | 2.3 s | veto: the summary of a pass must say the work is tested; skill, hook, prompt fragment and bin/ only in the attaching agent's session; a failed hook: the review plugin needs its require parameter |
| 16 | a bead: claim held through a crash, returned on failure, closed on pass, one run per bead | pass | 7.6 s | claim held through a crash; 5 requests for an epic of 3: 3 runs, 2 nothing_ready; out after 2 failures |
| 17 | a CLI command per API operation, the committed OpenAPI matches the code and real replies fit it, MCP serves its five tools | pass | 2.2 s | Summary [   0.532s] 6 tests run: 6 passed, 44 skipped |
| 17 | a session of the cheapest real claude model calls list_formulas on the daemon's MCP and reports it | not run | 0.0 s | needs a real model: run the spec without --ci |
| 18 | the journal rotates at 10 MiB x 5; a reader continues by seq; a rotated-away seq is a gap | pass | 1.5 s | rotated after 6 runs |
| 19 | operator gates stop dispatch, survive a restart and stay until opened by hand | pass | 2.0 s |  |
| 20 | the pack and both projects of demos/ load | pass | 0.0 s | `aae config check` of demos/config.toml is clean |
| 20 | demo 1, snake on claude, passes from empty rigs to a green smoke test without intervention | not run | 0.0 s | needs a real model: run the spec without --ci |
| 20 | demo 2, snake on claude and codex, passes from empty rigs to a green smoke test without intervention | not run | 0.0 s | needs a real model: run the spec without --ci |
| 22 | line budgets, MEASURE markers and a readiness file whose scores the open rows allow | pass | 0.2 s | budget: total      14616 / 30000 (hard stop 35000); readiness checked against the open rows |
| 23 | no todo!/unimplemented!, no unused public items or dependencies (the rest is review) | pass | 1.2 s | clippy (todo, unimplemented, unreachable_pub denied), cargo machete and scripts/unused-pub.sh are clean |

21 of 30 rows passed, 0 failed, 9 not run, in 91 s.

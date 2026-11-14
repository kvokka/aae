# aae acceptance spec

- date: 2026-11-14
- aae: aae 0.1.0
- claude: 2.1.325 (Claude Code)
- codex: codex-cli 0.168.0
- br: br 0.7.6
- host: Linux aarch64
- run: all

<!-- MEASURE id=acceptance-times date=2026-11-14 ver=aae-0.1.0 env=Linux-aarch64 src=aae-acceptance note="wall time of each row; the scenarios run concurrently, then the checks one by one" -->
| criterion | check | result | time | notes |
|---|---|---|---|---|
| 2 | a prompt step passes on the cheapest real claude model by the finisher | pass | 4.0 s | claude, model haiku |
| 2 | a prompt step passes on the cheapest real codex model by the finisher | pass | 6.6 s | codex, model gpt-5.6-luna |
| 4a | a finisher called with a background claude subagent running passes the run only after the subagent | pass | 37.1 s | claude, model haiku: the finisher came 31 s before the background subagent ended; the run passed after it |
| 8 | 20 x kill -9 of the daemon mid-step: up within 30 s, each run ends once, no firing lost or doubled | pass | 54.0 s | 20 kills (pause seed 3281512858519126910), 40 runs, 20 attempts failed by the crashes; slowest restart to the first answer 0.05 s |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real claude model: each run resumes and ends once, a passed one fires once | pass | 144.8 s | claude, model haiku: 5 kills mid-tool, 5 of 5 passed, 10 runs, 5 attempts failed by the crashes; slowest restart to the first answer 0.06 s |
| 8 | 5 x kill -9 of the daemon mid-tool on the cheapest real codex model: each run resumes and ends once, a passed one fires once | pass | 216.3 s | codex, model gpt-5.6-luna: 5 kills mid-tool, 5 of 5 passed, 10 runs, 5 attempts failed by the crashes; slowest restart to the first answer 0.09 s |
| 17 | a session of the cheapest real claude model calls list_formulas on the daemon's MCP and reports it | pass | 6.6 s | claude, model haiku: the session listed the formulas over MCP and reported "dev,zebra-audit" |
| 17 | a CLI command per API operation, the committed OpenAPI matches the code and real replies fit it, MCP serves its five tools | pass | 3.6 s | Summary [   0.108s] 5 tests run: 5 passed, 44 skipped |

8 of 8 rows passed, 0 failed, 0 not run, in 220 s.

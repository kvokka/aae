# Живая перепроверка на HEAD 5359011 (цикл 67)

Версии: claude 2.1.325, codex-cli 0.168.0, aae 0.1.0 (debug-сборка дерева, общий target/).
Модели: claude haiku; codex gpt-5.6-luna (дефолт сценария, AAE_CODEX_MODEL не задан); демо 1 — agent.toml: haiku.

## Критерий 8, строки «запрос»
`cargo run -q -p aae-acceptance -- --report /tmp/c67-acc.md 8` (1 прогон, 3 строки), всего 226 с (стенка 3:55 с учётом сборки).
- 20 x kill -9 в шаге (без модели): pass, 53.8 с
- 5 x kill -9 mid-tool, claude haiku: pass, 150.3 с (5 из 5 passed, 10 runs, самый медленный рестарт 0.06 с)
- 5 x kill -9 mid-tool, codex gpt-5.6-luna: pass, 225.9 с (5 из 5 passed, 10 runs, 0.07 с)
Отчёт: /tmp/c67-acc.md, вывод: /tmp/c67-acc.out.

## Демо 1
`cd /tmp/c67demo/demos && AAE=<repo>/target/debug/aae sh run.sh snake-claude` (копия demos/, снимок в репозитории не тронут), 1 прогон.
Результат: passed за 56 с от старта backend; run_1 backend, run_2 frontend, run_3 smoke — все passed.
RESULT.md: /tmp/c67demo/demos/snake-claude/RESULT.md. Демон run.sh остановлен (trap), в фоне ничего не осталось.
Tracked-файлы не менялись.

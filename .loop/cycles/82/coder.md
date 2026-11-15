# Цикл 82 — coder
STATUS: done

## Взято в работу
- WP-71 (демо 2, M); попутно жел. 1, 2, 4 ревью 81 и жел. 4 ревью 80 (номер раунда в `run show`).
- WP-72 (S, документация) — через subcoder, проверено и закоммичено мной.

## Сделано (коммиты)
- `5483941` WP-71: `round` в read-view прогона (`run show`, `GET /v1/runs/{id}`), contracts 1.3, openapi; тест в
  `returns.rs` проверяет `round = 2` после одного возврата.
- `6bbf1f1` WP-71: демо 2 `demos/2-multirepo-qa/`: пак `dev` (planner/coder/qa, все на codex), формула `deliver`
  = plan → work (drain) → `format` (exec, `rumdl check --fix` по путям всех ригов из `aae project list`,
  `back_to = "plan"`) → `qa` (prompt, `back_to = "plan"`), `rounds = 3`; условия `qa` — в `root/aae.toml`
  (`[rigs.root.vars] checks`), пак о задаче не знает (grep snake|score|game|frontend|backend|browser по `pack/` пуст).
  Модели: planner sol, coder luna, qa terra. `run.sh` пишет число раундов (`run show .round`) в `RESULT.md` и MEASURE.
  Acceptance: загрузка демо 2 (CI-строка 20) и строка «запрос» демо 2. `role_prompt.rs` копирует только `pack/` и
  `root/*.toml` (жел. 2). Прогоны в `RESULT.md` обоих демо сортируются по `created_at`, затем по id (жел. 4).
- `a6cab12` тест `a_step_gone_back_to_on_another_harness_starts_a_new_session` (жел. 1): `ScriptedDriver::named`
  играет второй харнесс на том же состоянии; reload между раундами переводит агента на `other`, вторая попытка `plan`
  — `new_session` на `other`. Мутация `=> own` в `machine.rs:173` теперь красная (проверил).
- `999861e` WP-72: mise необязателен (README, install.md, development.md — что ставить без него и какие команды за
  каждой задачей); зачем `root/config.toml` рядом с `aae.toml` — в `config.md` и `first-project.md`.
- `dce5cc2` WP-71: результат автора — `frontend-example/`, `backend-example/`, `RESULT-example.md` (чистый
  worktree на `6bbf1f1`, без `-dirty`), в README демо — сколько раундов понадобилось.
- `5b67b0f` READINESS: маркер `ver=a6cab12` (последняя правка `crates/`), текст про оба демо; баллы не менялись.

Живой прогон демо 2 (1 прогон, записи в журнале): **passed за 529 с, 1 раунд** — `format` и `qa` зелёные с первого
раза, `qa` реально играл в браузере (снимки до/после, счёт дошёл до бэкенда). Возврата вживую не было; сам возврат
покрыт без модели.

## Не получилось и почему
- Возврат к планировщику вживую не показан: проверки прошли в первом раунде. Критерий 20 («зелёные не позже третьего
  раунда») этим выполнен; условия проверки не ужесточал, чтобы не подгонять.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-72 стоит `doing`: «готово, когда» — ревьюер проходит гайды руками без mise.
- Хост: agent-browser на linux-arm64 не имеет Chrome for Testing; поставлен Chromium Playwright, путь — в
  `~/.agent-browser/config.json` (вне репозитория). README демо 2 об этом говорит.
- Следующий шаг: WP-73 (полная приёмка с моделями, перепрогон живых строк 2, 4а, 8 на шагах).

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — ждёт владельца (вопрос цикла 80 в журнале), ответа нет, код не менялся.
- Жел. 1 (тест фильтра по харнессу) — закрыто, `a6cab12`.
- Жел. 2 (`role_prompt.rs::copy`) — закрыто, `6bbf1f1`.
- Жел. 3 (`RESULT-example.md` демо 1 на `-dirty`) — не перепрогонял демо 1; демо 2 снято на чистом коммите. Остаётся
  для WP-73.
- Жел. 4 (сортировка прогонов) — закрыто в обоих `run.sh`.
- Жел. 5 (перенос из ревью 80): номер раунда в read-view — закрыто (`5483941`); `Step::Wait(1 s)` в `returns.rs`,
  `shortcut:`-пределы, `\n` в описании `passed_over` — не трогал.

## Проверки
`scripts/loop/checks 82` на `5b67b0f` (HEAD): pass — ci 71 с (nextest 453/453), acceptance 26 pass / 0 fail / 9 not run.

## Контекст на момент отчёта: ~140k

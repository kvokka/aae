# WP-83 · «Фон после исхода» · отчёт subcoder (цикл 100)

Сделано всё по плану, включая хвост 2.5 (шаг 5). Не закоммичено.

## Группы коммитов

Файлы целиком, каждая группа после предыдущих собирается. План делил claude/codex на отдельные коммиты, но
`stop_background` адаптеров нужен трейту в первом же коммите, а в `aae-codex/src/lib.rs` код шага 1 и хвоста лежит
вперемешку; поэтому адаптерные `src/` идут в первый коммит, во 2-й и 3-й — их тесты. Если хочется ровно по плану —
`git add -p crates/aae-codex/src/lib.rs` (хунки `stop_background` — в 1, хунки `tokens`/`bind`/`close_session` — в 5).

1. `WP-83: a declared outcome stops the commands a session left in the background`
   - crates/aae-core/src/driver.rs, crates/aae-core/src/run/{finish,events,exec}.rs
   - crates/aae-core/src/orphans.rs (`sweep_var` + тест — нужен codex lib.rs)
   - crates/aae-testkit/src/{lib,control}.rs
   - crates/aae-testkit/tests/core/{background.rs (новый), main.rs (+`mod background;`), finisher.rs (`pub(crate) fn finish`)}
   - crates/aae-claude/src/{lib,stream}.rs
   - crates/aae-codex/src/{lib,rpc}.rs
2. `WP-83: claude stops the shell tasks a session left` — crates/aae-claude/src/stream/tests.rs, crates/aae-claude/tests/claude.rs
3. `WP-83: codex cleans the background terminals of a thread and takes its detached commands with it` —
   crates/aae-codex/tests/codex.rs (оба теста: `the_commands_a_thread_left_...`, `a_detached_command_goes_with_its_thread`)
4. `WP-83: acceptance row 34 and the contracts of the background stop` —
   crates/aae-acceptance/src/scenarios/{c34_background.rs (новый), mod.rs (+mod, +элемент ALL), c04_finisher.rs (`events_of` → `pub(super)`)},
   docs/design/{contracts.md (4.1, 4.3, 4.6, 6.2 — одна новая строка после строки `finishing`, 6.3, 6.4), adapters.md, experiments.md}

## Проверки (всё зелёное)

- `cargo clippy -p aae-core -p aae-testkit -p aae-claude -p aae-codex -p aae-acceptance --all-targets -q -- -D warnings` — чисто
- `cargo nextest run -p aae-testkit` — 242/242; `-p aae-claude -p aae-codex` — 74/74; `-p aae-core` — 125/125
- приёмка: `cargo run -q --locked -p aae-acceptance -- --ci --report $R/report.md 34` — строка 34 `pass` (≈22 с,
  с отвязанными командами); вторая строка 34 — `open` из `OPEN`, её закрывает coder
- `scripts/budget.sh`: ядро 10 864 / 18 000, claude 898, codex 1 170; файлы ≤ 400 (events.rs 384, codex lib.rs 386,
  orphans.rs 382, scenarios/mod.rs 390)
- `rustfmt --edition 2024` по своим файлам

## Мутации (руками; все убиты, файлы возвращены)

| # | Мутация | Убил |
|---|---|---|
| 1 | events.rs без вызова | background №1, №4; приёмка 34 (`claude: A passed before its command ended`) |
| 2 | без `outcome IS NOT NULL` | background №2 |
| 3 | без `turn_ended != 0` | background №1, №3, №4 |
| 4 | claude `stopped` ставит `notified_since_result` | юнит + интеграционный claude |
| 5 | claude стопит и `local_agent` | юнит |
| 6 | codex `thread/unsubscribe` вместо `clean` | codex 2 треда |
| 7 | codex `clean` с чужим `threadId` | codex 2 треда |
| 8 | exec.rs без вызова драйвера | background №1, №2, №4 |
| 9 | без вызова из `finish()` | background №2 |
| хвост | codex `close_session` без `sweep_var` | `a_detached_command_goes_with_its_thread` |

Мутация 10 (сигнал вместо `stop_task`) руками не проверялась — её ловят `requests().len() == 3` и отсутствие
`TurnStarted` в интеграционном тесте claude по смыслу.

## [Д]-решения для журнала

- Сценарный драйвер по умолчанию — фон, который не останавливается (как субагент); `stoppable_background()`
  включает «команды». Старый тест finisher.rs теперь значит «неостанавливаемый фон ждётся».
- Хвост: шаг 2 опыта 8 (зачистка по токену) делается при `close_session` треда codex, а не в момент остановки;
  фильтра по session id нет — app-server токена не несёт. Если per-thread MCP-серверы codex несут токен треда, они
  тоже умрут при закрытии треда — это конец попытки, вреда нет (не проверено).
- `events_of` из c04 переиспользован (`pub(super)`), а не продублирован.

## Не сделано

- Ничего из плана не срезано. Запасной ход claude (Idle на пустом `background_tasks_changed`) не понадобился.
- Строки `"34"` в `OPEN`/`readiness.rs`/`checks.rs`/`READINESS.md` не трогал — за coder.

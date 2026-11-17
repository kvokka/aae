# Цикл 115 — coder
STATUS: done

## Взято в работу
Жел. 1–3 ревью 114 (Обязательных нет), WP-103 (план — expert, `.loop/cycles/115/notes/wp103-plan.md`), WP-104 (реализация — subcoder, проверка и коммит — я).
## Сделано (коммиты)
- `12afadc` — контрактный тест `run.awaiting` в `event_paths_follow_the_payload_schemas` (мутация: переименование ключа в `schema.rs:113` краснит тест); `docs/guide/first-project.md` перечисляет все пять видов шагов со ссылкой на «Formula».
- `2f1d7a2` — e2e `a_fail_from_the_cli_with_data_from_a_file_fails_the_run`: `aae run resolve … fail --data @file`, класс `agent`, исход с данными из файла.
- `90304c6` — WP-103: ключ `if` у шага любого вида (тот же `Filter`, что у `where`; пути проверяются при загрузке; `back_from` — имена проверок формулы), пропуск в `render` → `skip` → `advance` в той же транзакции, событие `run.step_skipped {run, step}` (закреплено в `event_paths_follow_the_payload_schemas`), колонки `runs.back_from` и `attempts.skipped` (схема 24), все шаги пропущены → `failed{infrastructure}`. Тесты: `aae-contract/src/tests/config_if.rs` (3), `aae-testkit/tests/core/step_if.rs` (6), `await_step.rs::a_restart_keeps_the_checks_that_sent_the_run_back`. Документы: config «Formula», contracts §1.3/§2.1–2.3/§6.1/§6.2, overview.
- `9080de1` — WP-103: `aae run watch` дописывает `step=<имя>`; e2e `a_skipped_step_shows_in_run_watch`; frontends.md.

Охрана → мутация → краснеет (все проверены):
| охрана | мутация | тест |
|---|---|---|
| `!step.condition.matches` в `render` | `false` | 5 тестов `step_if` |
| `row.step_attempt == 1 &&` | убрать | `a_repeat_of_a_step_does_not_ask_its_if_again` |
| `SET skipped = 1` в `skip` | `= 0` | `a_skipped_check_sends_nothing_back…` |
| `AND skipped = 0` в `advance` | убрать | тот же |
| `AND skipped = 0` в `step_results` | убрать | тот же |
| `back_from = ?3` | `'[]'` | `a_step_waiting_for_a_check…`, `a_restart_keeps…` |
| `if ran` у `run.step_finished` / `set_state` | `true` (каждое) | `a_step_waiting_for_a_check…` |
| ветка «нет исхода pass» в `advance` | `false` | `a_run_whose_every_step_is_skipped_fails` |
| `optional` в `prompt::path` (шаблон, step_inputs, step_if) | инверсия | `a_template_or_step_inputs…`, `the_paths_of_an_if…` |
| корни в `step_if`, `strangers` | убрать | `the_paths_of_an_if…` |
| `run.step_skipped` в `schema.rs` (и `run.awaiting`) | переименовать | `event_paths_follow_the_payload_schemas` |
| `step=` в watch | убрать | e2e `a_skipped_step_shows_in_run_watch` |
- `7109180` — WP-104 (реализация subcoder, проверка — я): `rounds` и `[inputs]` в конфиге оркестратора и `aae.toml`; слои в `aae-contract/src/config/defaults.rs` (формула > проект > оркестратор > 3; вызывающий > проект > оркестратор), проверка типа одна (`InputType::take`) на загрузку и старт, `task`-вход из конфига — ошибка загрузки, `required` из конфига — данный (`Formula::needs`), `explain --formula` с источником слоя. Код устаревших ключей перенесён из `files.rs` (лимит 400) в `retired.rs` без изменений. Тесты: `tests/config_defaults.rs`, `core/config_inputs.rs`, `returns.rs` (rounds из проекта), `api/tests.rs` (explain). Мутации subcoder: свой `rounds`, ветка `task`, тип, порядок слоёв, `needs` ×3, подстановка в `queue.rs`, обе строки explain; подстановку в `queue.rs` я перепроверил — краснеет `config_inputs::the_caller_beats…`. Допущения — журнал 11:10.

Гейт: `scripts/loop/checks 115` — pass на `7109180`, 37 s, nextest 589/589.

## Не получилось и почему
Нет.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующий пакет — WP-105 (демо 3, M).
## Замечания предыдущего ревью: что закрыто, что нет
Обязательных не было. Жел. 1 (`run.awaiting` не закреплён) — закрыто, `12afadc`. Жел. 2 — `--data @file` и `fail` через CLI закрыты e2e `2f1d7a2`; `run.preparing` при входе в `await` отдельным тестом не покрыт (путь общий). Жел. 3 — закрыто, `12afadc`. Жел. 4 (флейк chaos, `held` в codex, колонка `class`) — вне плана, не брал.
## Контекст на момент отчёта: ~150k

# Цикл 121 — coder
STATUS: partial

## Взято в работу
WP-113 (сам) и WP-112 (subcoder, параллельно). Жел. 1 и 2 ревью 120.

## Сделано (коммиты)
- `fc96816` WP-113 п.1 и п.3. Прогон, у которого пропущены последние шаги, кончается на шаге, который выполнился
  последним: `runs.step/agent/step_kind/step_attempt` восстанавливаются (поле `Row::ran` только в памяти, ставится в
  `enter` при выходе из выполненного шага, применяется в новом `finish.rs::ended`). `run show`/`run list` читают
  строку `runs` — видят этот шаг. Причина: `every step was skipped`, если не было ни одной попытки (`row.attempt == 0`);
  иначе (проверка вернула прогон в раунд, где всё пропущено) — `every step of round <n> was skipped`. Тесты в
  `step_if.rs`: `a_skipped_last_step_leaves_the_outcome_of_the_last_step_that_ran` (шаг в событии и в БД),
  новый `a_round_that_skips_every_step_fails_the_run`. Мутации: выключить восстановление — красный первый;
  `if row.attempt == 0` → `true` — красный новый; → `false` — красный `a_run_whose_every_step_is_skipped_fails`.
  Документы: contracts 1.3 (`step` завершённого прогона), contracts 6.2, config.md «Formula».
- `6dcc6d8` WP-113 п.2. `config explain --formula` печатает `# runs only if { "back_from" = "review", "inputs.n" = [4,5] }`
  перед `# a check: …`. Тест `a_formula_is_explained_with_the_layers_of_its_rounds_and_input_values` (aae-front);
  мутация `!is_empty()` → `is_empty()` краснит его. config.md (описание explain) поправлен.
- `c8d016e` WP-113 п.4. `still()` уже ждал вызовов фейкового `gh` (сделано раньше); оставшийся `sleep(SETTLE)` в
  проверке «на упавший прогон чистка не запускается» убран: прогон триггера коммитится в одной транзакции с событием
  (contracts 3.5), значит после `until_state(failed)` ждать нечего. `SETTLE` удалён. Мутация (в копии демо во
  временном каталоге добавлен триггер `run.failed` → `dev.cleanup`) краснит тест: 2 вместо 1. `demos/` не тронут.
- `1ede186` Жел. 2 ревью 120: contracts 6 (`finishing`) — у отказанного check-скрипта `failure_kind` попытки пуст.

Жел. 1 ревью 120: `scripts/core-boundary.sh` целиком — 0.14 s; четыре `cargo tree` не причина роста гейта, не менял.

## Не получилось и почему
## Начато и не закончено (файлы, точка остановки, следующий шаг)
WP-112 — у subcoder, жду.
## Замечания предыдущего ревью: что закрыто, что нет
Обязательных нет. Жел. 1 — замерено (0.14 s), не причина. Жел. 2 — оговорка в contracts 6. Жел. 3 — таблица
development.md не правилась в этом цикле, не трогал. Жел. 4 — WP-112.
## Контекст на момент отчёта: ~95k

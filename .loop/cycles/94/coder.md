# Цикл 94 — coder
STATUS: done

## Взято в работу
- Ревью 93: обяз. 1 (тесты триггера в корне), обяз. 2 (частичные строки приёмки, критерии 28–34, баллы по букве),
  жел. 1 (вариант/правка рига у агента `root`), жел. 2 (цена «шаг root не занимает корень» — в docs), жел. 3
  (`checks.rs` > 400 строк).
- WP-86 (S): тесты ядра `aae-testkit` в один бинарь.

## Сделано (коммиты)
- `74a63c0` WP-77: тест `config_triggers::a_project_trigger_without_a_rig_runs_in_the_root` — проектный триггер без
  `rig` в корне (`root` и `[root] name = "main"`), `rig = "main"` принимается, триггер пака на корень не назначается.
  Все три мутации ревью (`.or(root)`, `**r != root.0`, `pack.version.is_none()`) краснеют. Риг, который правит агента
  области `root` или назначает ему вариант, — ошибка конфига (`config/variants.rs`), тест
  `a_rig_neither_edits_nor_maps_an_agent_of_the_root`; фильтр `of_rig` в `compose.rs` стал не нужен и удалён (мутация
  проверки в `variants.rs` краснеет). `docs/design/config.md` — ошибка и цена общего корня.
- `18163c7` acceptance: список `OPEN` в новом `crates/aae-acceptance/src/readiness.rs` — части критериев, которые ни одна
  строка не проверяет (20, 24, 28–34); в отчёте строки `open`, прогон не валят, но критерий открыт для строки 22
  при любом выборе строк. Проверки готовности вынесены из `checks.rs` (413 → 277 строк). У критериев 28–34 есть
  строки в отчёте; тест `every_row_of_section_15` их требует. `docs/guide/development.md` — про `open`.
- `f5a7996`, `7aa1cd7` READINESS по букве: Runs and finisher, Recovery, Config assembly, claude, codex, br, Acceptance
  spec — 6; остальные 8; 10 нет (20 открыт). Строка 22 проверена руками: на старых баллах (маркер подменён на свежий)
  FAIL с перечнем, на новых pass.
- `1859c1e` WP-86: 33 файла → `tests/core/<имя>.rs` + `tests/core/main.rs`; `common` по-прежнему в `tests/common`
  (`#[path]`, как у chaos). Тела тестов не менялись, кроме переименования хелперов `impl World`, которые совпали по
  имени в одном бинаре: `exec` (start→start_run, end→run_end), `returns` (start_flow→start_returns, log→return_log,
  end→returns_end), `root` (start→start_in, turn→root_turn, pass→pass_turn), `beads` (finish→finish_bead). Ссылки в
  docs/демо/комментариях поправлены, все `aae-testkit/tests/*.rs` вне `.loop` указывают на существующие файлы.
  - `nextest list -p aae-testkit`: до и после по 212 тестов, имена совпадают с точностью до префикса модуля
    (`.loop/cycles/94/notes/wp86-before.txt`, `wp86-after.txt`). Бинарей: `aae-testkit` (lib), `chaos`, `core`,
    `bin/mcp-probe` — вместо 33+ тестовых.
  - Фильтр из `checks.rs` выбирает ровно `aae-testkit::core triggers::a_schedule_fires_on_the_managed_clock_and_once_after_downtime`.
- `0d7ee66` строки 27 (12 тестов) и 32 (6 тестов) гоняют новые тесты.
- Проверки: `scripts/loop/checks 94` на `7aa1cd7` — pass; nextest 481/481 (58,7 с), acceptance 30 pass / 0 fail /
  9 not run / 9 open.

## Не получилось и почему
- Ничего из взятого.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Следующий — WP-78 (M, критичный); закрывая части, убирать их из `OPEN` в `readiness.rs`.
- Старые тестовые бинари testkit в `target/debug/deps` остаются до `cargo clean` (не трогал).

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (тест + три мутации краснеют).
- Обяз. 2 — закрыто (`OPEN`, критерии 28–34 у компонентов, баллы по букве, строка 22 краснеет на старых).
- Обяз. 3 (`[root] vars`) — ждёт владельца, не трогал.
- Жел. 1 — закрыто ошибкой конфига (журнал: РЕШЕНИЕ 15:10, [Д] п. 3 цикла 93 поменян).
- Жел. 2 — цена записана в `docs/design/config.md`. Жел. 3 — `checks.rs` 277 строк.
- Жел. 4 (настенные часы в `root.rs`/`worktree.rs`), 5 (живой codex без git → WP-84), 6 (ревью WP-76) — не брал.

## Контекст на момент отчёта: ~120k

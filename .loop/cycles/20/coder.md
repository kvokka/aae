# Цикл 20 — coder
STATUS: done

## Взято в работу
- WP-19 «Трекер в ядре: владение бидом» (M, критичный). Expert в этом цикле недоступен (тип агента не найден) —
  план кодера: `.loop/cycles/20/notes/wp19-plan.md`.
- Жел. 5 ревью 19: тест ворот через `World::restart` (критерий 19).

## Сделано (коммиты)
- `5232990` WP-19: `aae-core/src/tracker.rs` (трейт `Tracker`: claim/release/close/ready, `Bead`, `TrackerError`),
  `run/beads.rs` (владение при создании, «следующий готовый» — выбор и запись владельца в одной транзакции, claim
  эффектом при `preparing`, промпт с `bead.*` после ответа claim, close при `passed`, условный release при
  `failed`/`cancelled`, проверка поколения у ответа claim, `tracker.sync_failed` с `next_retry_at`,
  `resync.diverged{bead}`, счётчик неудач → ворота `bead:` от `core`, сброс при открытии оператором), схема V8,
  конфиг `bead_failures`, ключ ворот `bead:` у прогона, `trigger.skipped{nothing_ready}`, ответ API
  `{"nothing_ready": true}`, `MemoryTracker` в aae-testkit, тесты `aae-testkit/tests/beads.rs` (5) и
  `gates.rs::gates_survive_a_restart_and_a_closure_with_until_still_opens_by_its_timer`; contracts 5.1, 6.1, 6.2,
  config.md, openapi.json.
- `9c1a107` план и READINESS.
- `3b25773` жел. 4 ревью 19: MCP-тест вызывает `run_state` в stateless-эпохе (проверены `resultType`, `_meta`) и
  передаёт `continue_from` через `start_run`.
- `70f3c31` WP-19: тест триггера с `{ next_ready = true }` — каждый бид своего рига один раз, затем
  `trigger.skipped{nothing_ready}` (ядерная часть 5а).
- `mise run ci` на `70f3c31`: fmt, clippy `-D warnings` чисто; nextest 175/175 (было 168); deny, machete,
  budget-selftest ok; budget.sh ok (ядро 6 486 / 12 000; `run/mod.rs` ровно 400 строк); EXIT 0.

## Не получилось и почему
- `BD_ACTOR` на попытку и путь БД в окружении сессии не сделаны: путь БД знает только адаптер br — перенесено в
  WP-31 (записано в плане). Демон пока запускается с пустой картой трекеров.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Жел. 1 (шапка READINESS) — закрыт: шапка и маркер на `5232990`.
- Жел. 4 (MCP stateless `tools/call`, `continue_from`) — закрыт `3b25773`.
- Жел. 5 (ворота переживают рестарт) — закрыт тестом в `tests/gates.rs`.
- Жел. 2, 3, 6 — остаются в своих пакетах (WP-21, WP-20b и др.).

## Что проверить ревьюеру
- Решения по [Д] — в журнале (запись цикла 20, РЕШЕНИЕ): владение при создании, а не при `preparing`; выбор
  «следующего готового» из сохранённого `ready_beads`; счётчик неудач только у прогонов, державших claim.
- Ключ очереди эффектов трекера — один на проект: недоступный трекер задерживает все операции проекта (в том числе
  старт-синк впереди claim) — так задумано, порядок claim → release важнее.
- Находка: двойной префикс `rejected: rejected:` в ошибках API (журнал).

## Следующий шаг
WP-31 (адаптер br: реализация `Tracker`, подключение в `daemon.rs`, наблюдатель → `Msg::TrackerChanged`, `bead.*`,
`BD_ACTOR`), затем WP-24 (каркас acceptance).

## Контекст на момент отчёта: ~195k

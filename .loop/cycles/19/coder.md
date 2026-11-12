# Цикл 19 — coder
STATUS: done

## Взято в работу
1. Обязательный 1 ревью 18: `cli/mod.rs` 407 строк > 400, CI красный.
2. Желательные 1–3 ревью 18 (contracts 3.5/6.1, строка в WP-22b); 4–6 разнесены в WP-21/WP-37/WP-29b.
3. WP-20 (восстановление) — разбит на WP-20a (сделан) и WP-20b (todo, зависит от WP-19/WP-31).
4. WP-25 (MCP-фронтенд) — сделан.

## Сделано (коммиты)
- `6b9d547` WP-23: группы команд `run`/`gate`/`trigger` вынесены в `crates/aae-front/src/cli/commands.rs`;
  `cli/mod.rs` 182 строки, `commands.rs` 253. Новые группы WP-22b ложатся туда же.
- `7d9d10e` WP-27a: флейк `a_process_that_dies_in_a_turn_reports_its_stderr_and_is_forgotten` — поддельный claude
  умирал до того, как `start` читал время старта (зомби → `HarnessCrash` на старте). Скрипт теперь умирает только
  после файла-сигнала. Поймано первым прогоном `mise run ci` этого цикла.
- `800c7cb` contracts 3.4/3.5: потребитель outbox один — журнал; триггеры и хуки привязаны к транзакции события.
  6.1: глубина 10 — фиксированный предел без настройки. План: жел. 3 → WP-22b, 4 → WP-21, 5 → WP-37, 6 → WP-29b.
- `1a2436f` WP-20a: `crates/aae-core/src/run/recover.rs` из `Handler::started` (§5.6, contracts 6.4):
  - каждый записанный экземпляр → `stopping` + эффект остановки по pid + время старта и сметание по маркеру,
    `instance.stopped{reason: recovery}` (причина остановки теперь идёт через эффект, раньше всегда `idle`);
  - прогоны в `starting`/`running`/`requires_action` → `interrupted` + `run.attempt_failed{daemon_crash}`; их
    `launch`/`prompt`/`respond` удаляются из очереди эффектов;
  - команда exec-шага получает случайный на попытку маркер `AAE_INSTANCE=x-<16 hex>`; при рестарте её exec-эффект
    удаляется, а под тем же ключом `exec:<run>` ставится сметание по маркеру — раньше любой следующей попытки;
  - `daemon.started{version, pid, interrupted_runs}` (число);
  - писатель перечитывает настенные часы не реже раза в 30 с (`RECHECK_MS`, замечание цикла 6 про сон машины).
  - Тесты `crates/aae-testkit/tests/recovery.rs` (3): рестарт посреди хода → `daemon_crash`, экземпляр остановлен,
    `new_session` продолжает на новом экземпляре; рестарт без прогонов ничего не трогает; команда exec-шага убита
    после рестарта. Мутация (снять маркер) ловится. `World::restart` в `tests/common` — абортит писателя и
    поднимает новый на том же состоянии с новым драйвером.
- `803fe77` план (WP-20 → split, WP-20a done, WP-20b todo) и READINESS (восстановление 2 → 4, `ver=1a2436f`).
- `28bae2c` WP-25: `crates/aae-front/src/api/mcp.rs` (179 строк) на `POST /mcp` (`GET` → 405) того же сокета;
  пять инструментов (`list_formulas`, `start_run`, `run_state`, `list_runs`, `cancel_run`) зовут обработчики API
  (`runs::create/get/list/cancel`), ошибки API → результат с `isError`; эпоха выбирается заголовком
  `MCP-Protocol-Version: 2026-07-28` (stateless, `server/discover`), иначе `initialize` без session id;
  `Api.config` для `list_formulas`; вне OpenAPI (не REST). Тесты `crates/aae/tests/mcp.rs` (2) на настоящем демоне;
  `docs/api/mcp.md`.
- `d4136a3` план (WP-25 done, MCP на TCP → WP-22b) и READINESS (MCP-фронтенд 0 → 4, `ver=28bae2c`).

Хвост `mise run ci` на `28bae2c` (последний коммит кода; дальше только план/READINESS):
```
     Summary [   4.594s] 168 tests run: 168 passed, 0 skipped
budget: core        6070 / 12000
budget: frontends   1168 / 4000
budget: total       9253 / 30000 (hard stop 35000)
EXIT 0
```
(fmt, clippy `-D warnings`, deny, machete, budget-selftest прошли — задача `ci` завершилась кодом 0.)

## Не получилось и почему
- Находка по ходу: id прогонов последовательные (`run_N`) и повторяются между демонами/тестами. Первый вариант
  маркера exec-шага `exec-<run>` убил команду параллельного теста `exec.rs` — отсюда случайный маркер. Записано в журнал.
- Субагенты в этой сессии недоступны (есть только типы `coder`/`reviewer`, хук запрещает `coder`) — всё сделано
  в своём контексте.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-20b (журнал доставки промпта `intent → sent → acked → settled`, проверка последнего неурегулированного ключа
  на resume после `daemon_crash`, resync) — описан в плане. Сейчас после рестарта reconcile шлётся всегда, а
  транскрипт проверяется ключом новой попытки (`run/exec.rs` `launch`, `run/machine.rs` `start_attempt`).
- Дренаж планового рестарта и `daemon.stopping` — в WP-22b (уже было там).
- MCP на TCP и проверка на живом харнессе — в WP-22b.
- Взятое в цикл закрыто; WP-20 разбит осознанно (20b зависит от WP-19/WP-31), поэтому STATUS done.
- Следующему кодеру: WP-19 (трекер в ядре, критичный) → WP-31 (br) → WP-20b; затем WP-22b (TCP, MCP на нём,
  перечитывание конфига с `arm_schedules`).

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыт (`6b9d547`), CI зелёный, хвост выше.
- Жел. 1, 2 — закрыты (`800c7cb`). Жел. 3–6 — внесены строками в WP-22b, WP-21, WP-37, WP-29b. Жел. 7 — без изменений.

## Контекст на момент отчёта: ~210k

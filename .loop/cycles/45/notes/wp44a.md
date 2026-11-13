# WP-44a: resume треда codex на живом app-server кончался `hollow_done`

## Причина (проверено трассой JSON-RPC, codex 0.168.0)

1. Первая попытка: ход падает (`usageLimitExceeded`), статус треда — `systemError`. `close_session` шлёт
   `thread/unsubscribe` → `unsubscribed`, но тред **остаётся загруженным** (ни `thread/closed`, ни
   `thread/status/changed{notLoaded}`; за 11 с в c10 и за 4 с опроса в опыте не выгрузился).
2. Вторая попытка: `thread/resume` с новым `config.shell_environment_policy.set.AAE_ATTEMPT_TOKEN`. app-server
   тред не перезагружает, а «присоединяется» к загруженному (в схеме: "If thread_id identifies a running thread,
   app-server rejoins that thread") — `config` молча игнорируется.
3. Команда хода `aae finish pass` выполняется со старым токеном: `aggregatedOutput: "stale_attempt\n"`,
   `exitCode: 1`. Поддельный API отвечает «Done.», ход кончается без решения → core шлёт «Your turn ended without
   an outcome» → то же самое → `failed{hollow_done}`. 0,2 с — это два честных хода на локальном поддельном API,
   а не чужой `turn/completed`: гипотеза кодера верна, но с уточнением.

Уточнение: дело в статусе треда. Простаивающий (`idle`) тред после `unsubscribe` тот же `thread/resume`
перезагружает сам (`notLoaded` → `idle`) и новый токен берёт — поэтому тест с успешным первым ходом был зелёным
на старом коде. Ломается именно повтор после **упавшего** хода, то есть ровно тот случай, когда повторы и бывают.

## Что пробовал (тест `a_thread_resumed_…`, временные ручки, в код не попали)

| Вариант | Итог |
|---|---|
| опрос `thread/unsubscribe` без флага | 40 × `notSubscribed` за 4 с, тред загружен, токен старый |
| только `-c thread_unload_delay_secs=0` | `thread/resume` → `-32600 thread … is closing; retry thread/resume after the thread is closed` |
| флаг + опрос `thread/unsubscribe` до `notLoaded` | зелёный: `unsubscribed` → через ~1 мс `notLoaded` + `thread/closed` → resume грузит с новым config |
| `thread/archive` + `thread/unarchive` перед resume | тоже зелёный и синхронно, но это злоупотребление архивом: двигает rollout (при `inherit_host` — в `~/.codex` владельца), падение между вызовами оставляет тред в архиве. Отвергнут |
| тред с идущим ходом, флаг + опрос | не выгружается (4 с `notSubscribed`) — отсюда предел ожидания |

## Фикс (ядро не тронуто)

- `crates/aae-codex/src/part.rs`, `args`: app-server стартует с `-c thread_unload_delay_secs=0` — последний
  `unsubscribe` закрывает тред сразу.
- `crates/aae-codex/src/lib.rs`, `resume_session`: перед `thread/resume` — `thread/unsubscribe` каждые 20 мс, пока
  не ответит `notLoaded`; предел `UNLOAD_WAIT` 10 с, дальше `transcript_divergence` (политика → `new_session`).
  На свежем app-server (после рестарта) первый же ответ — `notLoaded`, лишний вызов один.
- Нужны обе части: каждая по отдельности красная (см. таблицу). lib.rs — 391 строка.
- `docs/design/experiments.md` (строка «codex after resume in the same process») и `docs/design/contracts.md`
  (строка Resume таблицы 4.6) — по одной строке.
- `crates/aae-acceptance/src/scenarios/c10_quota.rs`: обход `instance_idle_s = 1` удалён.

## Тест

`crates/aae-codex/tests/codex.rs::a_thread_resumed_on_its_live_app_server_gets_the_new_attempt_token` — настоящий
codex, поддельный API: ход 1 падает по лимиту, `close_session`, `resume_session` с токеном `two`, команда хода 2
пишет `$AAE_ATTEMPT_TOKEN` в файл. Без фикса: `left: "one", right: "two"` (0,4 с); с фиксом зелёный, 30/30 под
`--stress-count 30`, 0,35 с, без sleep. Интеграционный в `crates/aae/tests/codex.rs` не добавлял: сквозной путь
держит c10.

## Проверка

- c10 без обхода: до фикса красный (`hollow_done`), после — 3/3 pass (11,4 / 11,1 / 11,8 с).
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo nextest run -p aae-codex -p aae`: 62/62.

## Открыто

- Побочный эффект флага: тред закрывается сразу при `close_session`, а не висит загруженным; судьба фоновых
  терминалов и MCP-серверов треда при закрытии не мерилась ([НП] шпаргалки 02 §6 остаётся).
- Гонка «`stop` прошлой попытки после `launch` следующей» (поздний `unsubscribe` снимет подписку новой попытки)
  была и до фикса; в трассах не встречалась, не проверял.
- Замечено попутно, не трогал: после сброса лимита `account/rateLimits/updated` приходит с `primary: null`, и
  `Stream::rate_limits` продолжает отдавать старое `utilization: 1.0` (разреженный снимок не чистится).
- Проверено только на codex 0.168.0; на 0.167.2 (нижняя граница `versions`) флаг и статусы не гонял.

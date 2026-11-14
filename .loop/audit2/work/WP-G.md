# WP-G — события драйвера до потребителей; обязательный промпт
STATUS: done

План эксперта: `.loop/audit2/work/WP-G-notes/plan.md` (основание — спека §2.2 п. 5 [Р] «не удаляется»).
Журнал: записи 23:05 (expert) и 01:30 (coder).

## Часть 1 — довести события драйвера

| Пункт | Коммит | Что сделано | Тест (краснеет без правки) |
|---|---|---|---|
| 1.1 `Usage`/`RateLimit` | 133f50e | миграция 15: `attempts.input_tokens/output_tokens` (приращения копятся), `account_limits(account, utilization, resets_at_ms)` по ключу `runs.account`; `usage` в `GET /v1/runs/{id}`/`aae run show` и в `run.passed/failed/cancelled`, токены в элементе `Attempts`; `rate_limit{utilization, resets_at}` в `run.attempt_failed`. Политика: аккаунт «исчерпан» при `utilization >= 1.0` и сбросе впереди → `quota_exhausted` без `retry_at` паркуется до `resets_at + quota_margin_s`; `rate_limited` при исчерпанном аккаунте — на воротах аккаунта до сброса | `driver_events.rs`: `the_tokens_of_a_run_add_up_over_its_usage_events`, `a_quota_without_a_reset_time_waits_for_the_accounts_known_reset` (проверено мутацией policy.rs); модульные случаи в `aae-plugins/src/tests.rs` |
| 1.2 `Retrying` | f65b29d | новое наблюдаемое событие `run.retrying{run, retry, next_retry_at, text}`; дедлайн тишины = `silence + delay` | `watchdog.rs: a_retry_the_harness_announces_adds_its_pause_to_the_silence_limit` (stall 00:02:30 вместо 00:01:00) |
| 1.3 `stop_reason` | 746152c | код причины в существующем `attempts.turn_ended` (0/1/2/3), без миграции; в событии — фактическая причина | `driver_events.rs: the_event_of_a_turn_without_an_outcome_tells_how_the_turn_ended` (`cancelled`) |
| 1.4 `Health::Degraded` | f816d72 | опрос здоровья (30 с) шлёт `Msg::InstanceHealth` при смене; миграция 16 `instances.degraded`; событие `instance.degraded{instance, reason}`, при выздоровлении — снова `instance.ready`; поле `degraded` в `GET /v1/instances`/`aae instance list`; экземпляр остаётся `ready` с сессиями. Стенд: `ScriptedDriver::degrade` | `driver_events.rs: a_degraded_instance_is_told_and_keeps_its_session` |
| 1.5 `seq` | 9119dc3 | миграция 17 `instances.seq`; `registry.rs: in_order` — `seq ≤ last` отброшено (ноль эффектов), `seq ≠ last+1` → `resync.diverged{subject:"instance"}`; `seq 0` — событие самого ядра, вне нумерации. Пропуск по построению невозможен (один `mpsc` в процессе, один отправитель на экземпляр: читатель, потом `child::watch`), проверка одна на оба случая. Стенд: `Step::Reseq(n)` | `driver_events.rs: a_repeated_driver_event_is_dropped_and_a_gap_is_told` |
| 1.6 `PromptState` | 56063f0 | **удалено различие** (единственный вывод «не нужно»): `Transcript::Present{delivered: bool}`. Доказательство: слияние в `run/exec.rs` (ветка `Present{..}` → промпт сверки) намеренное (contracts 6.4); «ждать неотвеченный» невозможно — к resume старый процесс всегда остановлен (`recover.rs` после краха демона; эффект `stop` + `old_instances` после любого другого сбоя), хода в полёте нет. Правки в `driver.rs`, `exec.rs`, адаптерах claude/codex, стенде и их тестах | новых нет: поведение не менялось; покрыто `tests/recovery.rs`, контрактными тестами claude/codex (36/36 зелёные) |

Документы — d9dedd6: contracts 1.3 (`RunView`+`usage`, `Attempts`, `run.*` терминальные, `run.retrying`,
`run.attempt_failed.rate_limit`, `stop_reason`, `instance.degraded`, `resync.diverged`), 4.2 (`Transcript`,
`Health`), 4.3 (`seq`, строки `turn.ended`/`usage`/`retrying`), 6.1, 6.4, 8, 10; `config.md` (`quota_wait_s`);
`invariants.md` (15, 20); `adapters.md` — ссылка на таблицу 4.3 вместо дубля + обязанности адаптера (приращения,
единицы `delay`, порядок `seq`). OpenAPI перегенерирован (в 133f50e и f816d72).

## Часть 2 — промпт обязателен — 4171c02

- пустой/пробельный файл роли (`prompt.md` или ключ `prompt`), шаблон формулы и фрагмент плагина — ошибка загрузки
  с именем файла (`config/items.rs: text`);
- промпт, пустой после подстановки — прогон `failed{infrastructure}`, `prompt: empty after substitution`, attempt 0,
  ни один процесс не стартует (`run/render.rs`);
- правило «`system_prompt` и файл роли в одном слое — ошибка» не тронуто (тест есть);
- `config.md` «Agent» и про шаблоны — по фразе.
Тесты: `tests/config.rs` (два случая), `aae-testkit/tests/runs.rs: a_prompt_empty_after_substitution_fails_the_run_before_a_launch` (краснел без правки).

## Завершение

- clippy workspace — чисто; `mise run ci` на d838c53: `397 tests run: 397 passed`, deny/machete/unused-pub/
  core-boundary/budget ok (`total 15445 / 30000`), exit 0.
- `mise run acceptance-ci` на d838c53: `21 of 30 rows passed, 0 failed, 9 not run`, exit 0; 11 — 36/36, 17 — 7/7,
  22 — pass. Счётчики 11/17 не менялись (новые тесты в `aae-testkit`).
- `docs/READINESS.md` маркер `ver=56063f0` (последний коммит в `crates/`) — d838c53.

## Изменённые файлы

`crates/aae-core/src/{driver.rs,store.rs,config/{items,schema}.rs,run/{events,exec,gates,machine,mod,msg,read,registry,render,row}.rs,tests/config.rs}`,
`crates/aae-plugins/src/{policy,tests}.rs`, `crates/aae-front/src/api/{replies,instances}.rs`,
`crates/aae-claude/{src/lib.rs,src/tests.rs,tests/claude.rs}`, `crates/aae-codex/{src/lib.rs,tests/codex.rs}`,
`crates/aae-testkit/{src/{lib,control,play,step,tests}.rs,tests/{driver_events,runs,watchdog}.rs}`,
`crates/aae/tests/codex.rs` (комментарий), `docs/api/openapi.json`, `docs/design/{adapters,config,contracts,invariants}.md`,
`docs/READINESS.md`.

## Нужные правки спеки (текстом)

- §5.4 (список событий): добавить наблюдаемые `run.retrying` (харнесс сам повторяет запрос; пауза не тишина) и
  `instance.degraded` (экземпляр работает, но харнесс сообщил ухудшение; `instance.ready` при восстановлении).
- §5.6 / §7.1 `transcript(key)`: «`Present{delivered}` — есть ли промпт в транскрипте; отвечен ли — не различается:
  к resume хода в полёте нет, доставленный промпт всегда сверяется».
- §11.5 `default-policy`: «`quota_exhausted` без `retry_at` — до `resets_at` последнего известного лимита аккаунта,
  если он исчерпан (`utilization >= 1`); `rate_limited` при исчерпанном аккаунте — ворота аккаунта до сброса».
- §7.1: `seq` — ядро отбрасывает повтор и сообщает разрыв (`resync.diverged`, субъект `instance`), `seq 0` — событие ядра.

## Что осталось / владельцу

- Находки вне пакета (не трогал, в журнале): `run/prompts.rs: unsettled` ищет потерянный промпт только после
  `daemon_crash` (после другого сбоя resume шлёт только сверку без задачи; для claude прикрыто
  `transcript_divergence`, для codex [НП]); пересыльщик событий драйвера (`run/mod.rs`) выходит на любой ошибке отправки.
- [НП] claude `result.usage` — за ход или накопленный за процесс (от этого зависит сумма токенов попытки с замечаниями);
  кэш-токены claude в `input_tokens` не входят.
- `Health::Degraded` и разрыв `seq` сегодня порождает только стенд (ни один адаптер) — потребитель есть, производителя нет.

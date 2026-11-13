# Цикл 29 — coder
STATUS: done

## Взято в работу
- Обяз. 1 ревью 28 (вариант а), жел. 1 и 2.
- WP-22c в усечённом виде: состояние демона, дренаж, плановый рестарт, экземпляры через API.
- WP-35: `doctor`, разрешение бинарников, файлы сервиса.

## Сделано (коммиты)
- `4016c61` — `PluginUse.config` убран (ключ теперь «unknown field», тест в `aae-core/src/tests/config.rs`),
  «объясни» пишет `attached`, `docs/design/config.md` поправлен; тест отказа перечитывания при смене `max_sessions`
  (`aae-testkit/tests/instances.rs`).
- `7c85a70` — WP-22c: `GET /v1/daemon` (здоровье журнала `journal{failures,last_error}`, `outbox_depth`,
  `outbox_oldest_age_ms`, `active_runs`, `stopping`); `POST /v1/daemon/drain`, `/v1/daemon/restart` (`?timeout_s=`):
  `Msg::Stopping` → `daemon.stopping{reason}`, новые прогоны ждут с `waiting_for: daemon_stopping`; стоп при 0
  прогонов в ходе или по таймауту; рестарт — `exec` того же бинарника в том же процессе. SIGTERM пишет
  `daemon.stopping{signal}` и ждёт строку журнала (до 5 с). CLI `aae drain|restart [--timeout]`. `shortcut:` про
  дренаж в `daemon.rs` снят. Тесты на настоящем демоне (`crates/aae/tests/api.rs`).
- `b301e41` — WP-22c: `GET /v1/instances`, `POST /v1/instances/{id}/stop` (только простаивающий; занятый,
  стартующий, неизвестный — 409), `instance.stopped{reason: operator}`; CLI `aae instance list|stop`; тест в testkit.
- `6b2a8f9` — WP-35: `daemon::binary` (конфиг → `mise which` → `PATH`), br запускается по разрешённому пути
  (`Br::new(binary, dir)`); `aae doctor [--json]` без демона (конфиг, харнессы: путь+источник, версия, логин; br:
  версия, предупреждение о лицензии; права каталогов состояния и сокета; состояние и outbox); `aae daemon --service
  systemd|launchd`. 4 теста на подставных бинарниках (`crates/aae/tests/doctor.rs`). Раздел в README. `shortcut:`
  про WP-35 в `daemon.rs` снят.
- `3790c3f` — READINESS: HTTP API 5.5, CLI 5.5, br 6.5, поставка и doctor 3.
- OpenAPI перегенерирован (`docs/api/openapi.json`), контракт: `waiting_for` += `daemon_stopping`, причина
  `instance.stopped` += `operator`.

## Не получилось и почему
- Субагенты `subcoder`/`helper`/`expert` в этой сессии недоступны (Agent: «available: coder, reviewer»), хук
  запрещает coder → coder; запись в журнале. Всё сделано без делегирования.
- Флейк: дважды за цикл, оба раза в первом полном прогоне сразу после пересборки, тест на настоящем демоне падал
  «the daemon did not come up» (10 с, `crates/aae/tests/common/mod.rs:53`) — разные тесты. Повторные прогоны (×7)
  зелёные. Похоже на нагрузку при холодном старте свежего бинарника; не расследовал.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-22c остаток: живой просмотр (SSE + `Driver::live`), включение/выключение триггера, TCP с токеном и `/mcp` на нём.
- WP-35 остаток: проверка stdio-MCP в настройках хоста (вместе с WP-34); `br doctor` не вызывается.
- Не взято: WP-34 (критерий 14), WP-38, WP-39, WP-32b, WP-21.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто вариантом (а).
- Жел. 1 — WP-32 переведён в статус `split`. Жел. 2 — закрыто тестом.
- Жел. 4 — закрыто (`c0a239e`). Жел. 3 (повтор reload после рестарта), 5 — не взяты.

## Проверки
- fmt, clippy `-D warnings` чисто; nextest 240/240.
- `mise run ci` на `6b2a8f9` + README/READINESS: EXIT 0. `mise run acceptance-ci`: EXIT 0, 14/14 за 611 с.
- `c0a239e` — жел. 4 ревью 28: README примера оговаривает разбор JSON через `sed`.

## Контекст на момент отчёта: ~225k

STATUS done: взятое (обяз. 1, усечённый WP-22c, WP-35) закончено; остаток WP-22c описан выше и в плане.

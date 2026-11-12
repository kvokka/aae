# Цикл 12 — coder
STATUS: done

## Взято в работу
1. Обязательный п. 1 ревью 11 (ворота аккаунта до `launch`) + желательные 1, 2.
2. WP-22 → разбит на WP-22a (сделан) и WP-22b (todo).
3. WP-23 (CLI) на поверхности WP-22a — сделан.

## Сделано (коммиты)
- `c43cde6` WP-16: `start_attempt` вычисляет `split` и `Driver::account` до записи попытки; закрытые ворота
  `account:` паркуют без `launch`. `ScriptedDriver::account` берёт `account` из native-блока. Закрытие с `until` в
  прошлом: оператор — `rejected`, решение — пусто. contracts 7 поправлен. Тесты в `tests/gates.rs`.
- `a4245e8` WP-22a: `aae_core::run::{Reader, RunFilter}` (чтение отдельным read-only соединением, общий маппинг с
  писателем), `Journal::subscribe`, удержание ключей идемпотентности 24 ч (схема v5; закрыт `shortcut:`).
- `e30dc17` WP-22a: крейт `crates/aae-front` (axum + utoipa): `/v1/runs` (+ `{id}`, `cancel`, `answer`, `finish`),
  `/v1/gates`, `/v1/gates/open`, `/v1/events` (SSE с `from`/`Last-Event-ID`, разрыв → 410 `gap` с `oldest`),
  `/v1/daemon`; `Idempotency-Key`; ошибки `{"error":{code,message}}`; OpenAPI `docs/api/openapi.json` + тест
  расхождения (`AAE_UPDATE_OPENAPI=1`); клиент по unix-сокету. `aae daemon` (XDG, `--config`, `--socket`, каталог
  0700, второй демон на живом сокете не стартует, SIGTERM/SIGINT удаляет сокет). 5 интеграционных тестов на
  настоящем бинарнике (`crates/aae/tests/api.rs`).
- `c8ea044` READINESS: ver=e30dc17; цикл событий 3→4, прогоны 3→4, ворота 3→4, HTTP API 0→3.
- `7c52070` WP-23: CLI (clap) — `aae run start|list|show|cancel|answer`, `aae gate list|close|open`, `aae events`,
  `aae finish` (`AAE_RUN`/`AAE_ATTEMPT_TOKEN`, ретрай 60 с → код 75), `aae status`, `aae daemon`; `--json`,
  `--socket`/`AAE_SOCKET`; таблица `cli::OPERATIONS` сверяется с OpenAPI тестом (критерий 17, часть CLI);
  XDG-пути вынесены в `aae_front::dirs`; 3 интеграционных теста CLI на настоящем демоне.
- `3152014`, `c6018fc` READINESS: HTTP API 3→4, CLI 0→4, ver=7c52070.
- `mise run ci` зелёный: 108 тестов, deny/machete ок; бюджет: ядро 5087, фронтенды 897, всего 6114 / 30000.

## Не получилось и почему
- Субагенты недоступны (`subcoder`/`helper`/`expert` не найдены, `coder` запрещён хуком) — всё в основном
  контексте; запись в журнале.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего начатого не брошено. Дальше по плану:
- WP-22b целиком (поверхность конфига, перечитывание, дренаж/рестарт, живой поток, TCP+токен, статус журнала).
- Известные `shortcut:` WP-22a: остановка демона без дренажа (`crates/aae/src/daemon.rs`), SSE читает журнал с
  начала файлов (`crates/aae-front/src/api/events.rs`).
- Путь финишера «демон недоступен 60 с → код 75» тестом не покрыт.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательный п. 1 — закрыт (`c43cde6`).
- Желательный 1 (`until` в прошлом) — закрыт; 2 (READINESS ver) — закрыт; 3 (порядок `parked`/`queued`) — нет;
  4 — хвосты WP-21/27/33 без изменений.

## Контекст на момент отчёта: ~210k

# Цикл 35 — coder
STATUS: done

## Взято в работу
1. Обяз. 1 ревью 34: актуальность `docs/READINESS.md`.
2. Остаток WP-22c: живой просмотр прогона (SSE + `Driver::live`), команда CLI; TCP по явному включению (loopback +
   токен), `/mcp` на нём с вызовом из сессии claude и codex.

## Сделано (коммиты)
- `aaec505` docs: READINESS на `1708893` — шапка и `ver=`, «цикл событий» и «триггеры» 8 с «missing for 9: real
  owner runs (criterion 20)», у каждой строки теперь есть «missing for» (добавлено у хуков плагинов и br).
- `75d633f` WP-22c: живой просмотр.
  - `GET /v1/runs/{id}/live` (`crates/aae-front/src/api/live.rs`): события прогона из журнала с `seq`
    (`from`, `Last-Event-ID`, по умолчанию с самого старого); сырые сообщения текущей сессии из `Driver::live` как
    `event: harness` без `id`; `lagged` с `{"missed": n}`; поток кончается на `run.passed|failed|cancelled`.
    Сессия новой попытки подхватывается: после чтения журнала и раз в 500 мс.
  - В состоянии прогона у каждой попытки есть `harness` и `session` (ссылка на сессию, §7.7).
  - `aae run watch <run> [--from]`; вывод `aae events` и `run watch` идёт через общий `follow`.
  - `Api.drivers` — драйверы из демона. OpenAPI перегенерирован; строка в `cli::OPERATIONS`.
  - Тесты:
    - `crates/aae/tests/claude.rs::the_live_view_of_a_run_relays_its_session_and_ends_with_the_run` — настоящий
      claude на подставном API: события только своего прогона от `run.queued` до `run.passed`, есть `harness`
      с `type=user`, у попытки есть `session_id`;
    - `cli.rs`: `aae run watch` на законченном прогоне печатает события и завершается.
  - Гайды: `docs/guide/first-formula.md`, `first-project.md`.
- `2acfafd` WP-22c: TCP и MCP из сессии.
  - `api_tcp` в конфиге оркестратора: только loopback, иначе ошибка; перечитывание отказывает, если ключ меняется.
  - Второй слушатель в `api::serve`, на нём middleware `Authorization: Bearer`: сравнение за постоянное время,
    без токена — 401 JSON.
  - Токен новый на каждый старт демона, пишется в `<state>/api.token` с правами 0600. В ядро попадает через
    `Config.api_token`, его ставит демон, в том числе при перечитывании.
  - `${AAE_API}` в `url` MCP → `http://<api_tcp>` + Bearer (`run/services.rs::bind_services`). Без `api_tcp`
    сервер у сессии пропадает.
  - Тесты:
    - конфиг: loopback — да, `0.0.0.0` — ошибка;
    - claude: 401 без токена и с чужим токеном, 200 с токеном; сессия вызывает `mcp__aae__list_formulas` по TCP,
      и результат приходит модели;
    - codex: тред вызывает `list_formulas` по TCP.
  - Чтобы уложиться в лимит 400 строк: `wait_up` перенесён из `services.rs` в `exec.rs`, `parent`/`resolve` — из
    `config/load.rs` в `config/items.rs`.
  - Доки: `docs/design/config.md`, `docs/api/mcp.md`.
- `e30d2c2` docs: READINESS на `2acfafd`: HTTP API 7, CLI 7, MCP-фронтенд 6.
- `mise run ci` на `2acfafd`: EXIT 0, nextest 265/265, budget всего 12432/30000. `acceptance-ci` не гонял: ядро
  меняется только в `bind_services` (ветка `${AAE_API}`), сервисы покрыты `aae-testkit/tests/services.rs`.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ветки `lagged` и `gap` живого просмотра тестами не покрыты (записано в READINESS как missing for 8).
- `shortcut:` в `live.rs`: если конец законченного прогона уже вытеснен из журнала, поток не закрывается сам
  (журнал, цикл 35).
- WP-22c закрыт. Следующее по ревью: WP-29b, WP-32b, WP-20c, WP-36, затем WP-39.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 (READINESS) — закрыто: `aaec505`, а после работы цикла ещё `e30d2c2` (шапка, `ver=2acfafd`, баллы
  согласованы с «missing for»).
- Жел. 4 (событие после N неудач дедлайна) — закрыто решением [Д] в журнале: хватает лога в stderr.
- Жел. 1 (время `check` на GitHub) — проверить после пуша этого цикла, не сделано.
- Жел. 2, 3 (тестовый `aae-testkit/src/lib.rs` 423 строки), 5, 6 (`triggers_off` после удаления триггера) — не
  трогал.

## Контекст на момент отчёта: ~195k

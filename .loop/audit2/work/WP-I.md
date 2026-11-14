# WP-I — прогон, запущенный из сессии агента: дочерний прогон и ожидание исхода
STATUS: done

## 1. Дочерний прогон — сделано (3654a81)

- `Create` получил `token: Option<String>` (`run/msg.rs`); `RunHandler::child_of` (`run/queue.rs`): токен
  проверяется как у финишера — попытка с этим `token_sha256` должна быть текущей и живой (`attempt_live`) у
  `parent_run`, иначе `rejected` «stale attempt»; ребёнок берёт correlation родителя, `depth + 1`; больше 10 —
  `rejected` «depth 11 is over 10». Предел — одна константа `MAX_DEPTH` (`run/triggers.rs`, теперь `pub(super)`).
- API: `POST /v1/runs` принимает `parent: {run, token}` (`api/runs.rs`, `ParentAttempt`); без него — как раньше
  (новый correlation, глубина 0). OpenAPI перегенерирован.
- CLI `aae run start` сам берёт `AAE_RUN` + `AAE_ATTEMPT_TOKEN` из окружения, как `aae finish`.
- Родитель виден: `parent_run` и `depth` уже были в `RunView` (`GET /v1/runs/{id}`, `aae run show`, данные
  `run.queued`). Дети — фильтр `aae run list --parent <id>` / `GET /v1/runs?parent=` (`RunFilter.parent`) —
  меньше кода, чем список в представлении. Фильтр отдаёт всех с этим `parent_run`, включая запущенных триггером.
- MCP `start_run` не расширен: сессия MCP не несёт попытку агента (проект §5: MCP не расширяется). Поле `parent`
  в теле tool-вызова десериализуется тем же `CreateRun`, но в схеме инструмента его нет; описано в `docs/api/mcp.md`.
- Тесты: `crates/aae-testkit/tests/children.rs` (ребёнок: correlation, `parent_run`, depth 1; поддельный токен;
  токен без run; глубина 11 отвергнута); `crates/aae/tests/wait.rs::a_start_in_a_session_carries_its_attempt_...`
  (CLI шлёт окружение — устаревшая попытка отвергается; без окружения — depth 0, `run list --parent`).
- Во всех литералах `Create` в `aae-testkit/tests/**` добавлено `token: None`.

## 2. `aae run wait` — сделано (59560e2)

- `crates/aae-front/src/cli/wait.rs`: сначала открывается `/v1/events` (только новые), потом читаются состояния —
  конец между ними не теряется; дальше ждёт `run.passed|failed|cancelled` своих прогонов и читает их представление.
  Не опрос. Обрыв потока / Io / 503 — переподключение с backoff (250 мс → 5 с) и перечитывание состояний; `gap` —
  то же. Вывод: `{id, state, class, reason, summary, data}`, с `--json` массив, без — `key: value` блоки (общий
  `print`). Коды: 0 — все `passed`; 1 — нет (или неизвестный прогон, ошибка API); 124 — истёк `--timeout`
  (печатается текущее состояние каждого). `--timeout` парсится `aae_core::config::duration`.
- Новой операции API не понадобилось (поток событий + `GET /v1/runs/{id}`), `OPERATIONS` не меняется.
- Тесты `crates/aae/tests/wait.rs`: исходы и коды 0/1, текстовый вывод, неизвестный прогон; таймаут → 124 и
  `running`; рестарт демона (SIGTERM + новый демон) посреди ожидания → ожидание дожидается терминального состояния.

## 3. Каскад отмены — сделано (в 3654a81), ~14 строк

`cancel` (`run/events.rs`) после терминального перехода отменяет живых детей с `parent_run = run AND trigger IS
NULL`, причина `parent_cancelled`, рекурсивно (внуки тоже). Прогоны, запущенные триггером на событие родителя, не
трогаются — у них тоже `parent_run`, но это не «запущенные родителем». Провал/успех родителя детей не трогает.
Тест: `children.rs::cancelling_the_parent_cancels_its_live_children_and_stales_its_token` — краснеет без правки
(проверено обратным патчем).

## 4. Ожидание и сторож — находка, код сторожа не менялся

- Порог при идущем инструменте — `[watchdog] tool_silence` (умолч. 10 мин), продлевается каждым
  `Activity{tool_running:true}` (`run/events.rs`); шаг целиком ограничен `[step] timeout` (умолч. 45 мин).
- claude: `tool_progress` раз в 30 с → `Activity{tool_running:true}` → сторож не сработает. Но claude сам уводит
  foreground Bash в фон через 120 с (`BASH_DEFAULT_TIMEOUT_MS`, шпаргалка 06) — дальше ход живёт по правилам фоновой
  работы (`background_silence`, 30 мин).
- codex: пульса нет; `Activity` — только на `item/started|completed` команды и на `*/delta` (вывод). `aae run wait`
  молчит до конца → ожидание дольше `tool_silence` = `stall`, раньше таймаута шага (45 мин).
- Рецепт (в `docs/api/frontends.md` рядом с `run wait`): цикл `aae run wait <ids> --timeout 100s` (124 — ждать
  снова) — работает для обоих харнессов; либо поднять `tool_silence` выше самого долгого ожидания.
- Таймаут команды codex (`commandExecution`) в шпаргалках не описан — не проверял.

## 5. Документы — сделано (4e7f6f5)

`docs/api/frontends.md` (run start в сессии, `--parent`, `run wait`, коды, сторож), `docs/api/mcp.md` (start_run без
попытки), `docs/design/contracts.md` (correlation в конверте, `depth`/`parent_run` в RunView, §6.1 — дочерний прогон,
предел, каскад отмены; строка отмены в таблице §6.2). `event-model.md` не менялся: данные событий те же (`parent_run`
и `depth` уже были в `RunView`).

## Изменённые файлы

crates/aae-core/src/run/{events,msg,queue,read,triggers}.rs; crates/aae-front/src/api/runs.rs;
crates/aae-front/src/cli/{commands,mod,wait}.rs; crates/aae-testkit/tests/{children.rs (новый), chaos/steps.rs,
common/beads.rs, common/mod.rs, exec.rs, instances.rs, recovery.rs, restart_decisions.rs, runs.rs, trigger_scope.rs,
triggers.rs, worktree.rs}; crates/aae/tests/wait.rs (новый); docs/api/{openapi.json,frontends.md,mcp.md};
docs/design/contracts.md. Журнал: две записи (РЕШЕНИЕ, НАХОДКА).

## Нужные правки спеки (текстом, для исполнителя документации)

- §9.5 / §12: «Прогон, запущенный `aae run start` из сессии агента (в окружении `AAE_RUN`, `AAE_ATTEMPT_TOKEN`), —
  дочерний: correlation родителя, глубина +1, общий предел 10; отмена родителя отменяет его живых детей
  (`parent_cancelled`)». И команда `aae run wait` в перечне CLI (коды 0/1/124).
- Критерий 15/acceptance: счётчики 11 и 17 не затронуты (новые тесты не в их наборах).

## Что осталось

- Живой проверки с настоящим харнессом нет (запрещено): планировщик на claude/codex в демо проверит рецепт ожидания.

## Проверки перед сдачей

- clippy `aae-front`, `aae-testkit`, `aae` (+ `aae-core` как зависимость) `-D warnings` — чисто, кроме чужого
  `clippy::large_enum_variant` в `aae-core/src/config/mod.rs` (WP-H посреди правки; прогнан с `-A` на эту линту).
- nextest `aae-front`, `aae-testkit`, `aae`: 229/231; два падения — `aae/tests/doctor.rs` (зона WP-H, посреди правки).
- `aae-core` lib-тесты не собираются из-за `aae-core/src/tests/config.rs:148` (зона WP-H); мои правки в
  `aae-core` покрыты `aae-testkit/tests/children.rs` (3/3) и `aae/tests/wait.rs` (3/3).

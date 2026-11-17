# WP-94 · план реализации: разрешения решает харнесс

Три группы с непересекающимися файлами (A — контракт, ядро, фейковый драйвер; B — адаптеры; C — фронт и документы),
плюс короткий шаг 0 до них и интеграция после. Номера строк — на HEAD 977d29b; ориентир — символ, а не номер.

## 0. Общее для всех

### Что проверено при составлении плана (2026-11-17)

- `claude --permission-prompts <host|none>` есть в 2.1.322, 2.1.325 и 2.1.326 (`--help` всех трёх установленных
  версий); режимы `--permission-mode`: `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan`
  (строгий режим «по умолчанию» называется `manual`, слова `default` среди вариантов нет).
- codex 0.168.0, `codex app-server generate-json-schema`: серверные запросы — `item/commandExecution/requestApproval`,
  `item/fileChange/requestApproval`, `item/permissions/requestApproval`, `item/tool/requestUserInput`,
  `mcpServer/elicitation/request`, `item/tool/call`, `account/chatgptAuthTokens/refresh`, `attestation/generate`,
  `applyPatchApproval`, `execCommandApproval`. Ответ `{"decision":"decline"}` существует только у
  `commandExecution` и `fileChange`; у `item/permissions/requestApproval` ответ — `{permissions, scope}`, поля
  `decision` нет. Поэтому: `decline` — на эти два, JSON-RPC-ошибка — на все остальные (отступление от буквы пакета
  «decline на `item/*/requestApproval`», в журнал).
- `crates/aae-testkit/src/{anthropic,openai,mcp}.rs` попали в grep только из-за своей локальной `fn respond` (HTTP-ответ
  фейка). Их никто не меняет; если адаптерным тестам понадобится новый `Reply` — файл принадлежит группе B.
- `crates/aae-acceptance` правок не требует: совпадения там — только `approval_policy = "never"` в конфигах сценариев
  (остаются валидными). Фильтры `checks.rs` считают тесты: `contract::`/`models::` — 41 (строка 76), `replies::` +
  openapi — 8 (строка 90). Число тестов в этих модулях не менять (переписывать на месте).
- chaos-стенд запросов разрешений не играет; правится только `tests/chaos/check.rs` (группа A).

### Интерфейс: что A удаляет, на что B и C перестают опираться

| Удаляется (A) | Кто опирался |
|---|---|
| `aae_contract::driver::Answer` (enum `Allow{updated_input}` / `Deny{message}` / `Text{text}`) | B: `aae-claude/src/lib.rs`, `aae-codex/src/lib.rs`, оба `tests/harness/contract.rs`, `aae-codex/src/tests.rs` |
| `aae_contract::driver::RequestKind` (enum) | B: оба `stream.rs`, `aae-claude/src/stream/tests.rs`, `aae-codex/src/tests.rs` |
| `Normalized::PermissionRequested { request_id, kind, tool, input, text }` | B: оба `stream.rs`, `aae-claude/src/lib.rs:151`, `aae-codex/src/rpc.rs:168`, тесты |
| `SessionState::RequiresAction` (вариант; остаются `Running`, `Idle`) | B: `aae-claude/src/stream.rs:113`, `aae-codex/src/stream.rs:136` |
| `Driver::respond(&self, instance: &InstanceHandle, session: &SessionRef, request_id: &str, answer: &Answer) -> BoxFuture<Result<(), Failure>>` | B: `impl Driver` обоих адаптеров |
| `aae_contract::config::Watchdog::human_timeout_ms`, ключ `[watchdog] human_timeout` | C: `demos/*/root/config.toml`, документы |
| `aae_core::run::Msg::Answer { run, request, answer }` | C: `aae-front/src/api/runs.rs:182` |
| `pub fn aae_core::run::answer_from(&Value) -> Option<Answer>` | C: `aae-front/src/api/runs.rs:7,180` |
| `aae_core::run::Decision::Answer(Answer)` | никто вне ядра и его тестов |
| поле `request` в `RunView` (`read.rs`), состояние `requires_action`, событие `run.requires_action`, эффект `respond`, колонка `attempts.request` | C: `aae-front/src/api/replies.rs:51-54`, `runs.rs:79` |

`SessionState::RequiresAction` в пакете не назван, но уходит: после правки его никто не порождает и не читает, а
строка `"requires_action"` в `aae-claude/src/stream.rs` нарушает критерий grep.

### Порядок и сборка

- Рабочее дерево одно, `target/` один: cargo исполнителей идут по очереди (блокировка каталога сборки) — это нормально.
- **Шаг 0 делает coder сам (или A первым делом, до запуска B и C)** — раздел A, пункты A1–A2: весь `aae-contract` и
  фейковый драйвер `aae-testkit/src/{lib,tests}.rs`. Проверка: `cargo check -q -p aae-contract && cargo check -q -p
  aae-testkit --lib`. После него `aae-contract` и библиотека testkit окончательны, а `aae-core`, адаптеры и фронт не
  собираются, пока группы не закончат.
- B собирается сразу после шага 0 (`aae-claude`, `aae-codex` зависят только от `aae-contract` и библиотеки testkit).
- C: `aae-front` зависит от `aae-core`, поэтому C сначала правит документы и демо, затем файлы фронта, а cargo
  запускает, когда `cargo check -q -p aae-core` зелёный (ждать событием, предел 20 минут; не дождался — сдать правки
  без прогона и сказать об этом).
- Исполнители не коммитят (промежуточные состояния не собираются). Коммитит coder после интеграции (раздел Z).
- Форматирование — только свои крейты: `cargo fmt -p <crate>`.

### Записи в журнал (делает coder; по одной записи РЕШЕНИЕ/ВОПРОС)

1. Критерий `git grep -nE 'requires_action|human_timeout|run answer'` выполняется с двумя исключениями:
   `docs/agents/decisions.md:40` (текст решения 116 владельца называет ушедшее) и сырые логи `experiments/*/logs`.
2. `item/permissions/requestApproval` получает JSON-RPC-ошибку, а не `decline` (схема codex 0.168.0, см. выше).
3. ВОПРОС: дефолт (`bypassPermissions`, `approvalPolicy = "never"`) отменяется только конфигом aae
   (`native.claude.permission_mode` или `native.claude.permissions.defaultMode`; `native.codex.approval_policy`).
   Настройка хоста при `inherit_host` дефолт не отменяет — в отличие от `sandbox_mode` (решение 38 смотрит и хост).
   Принято так по тексту пакета и решению 120 (слои: default → orchestrator → project → formula).
4. `demos/1-planner-executor/README.md:50-52` правится вместе с `config.toml` (там назван `human_timeout`) — в рамках
   разрешения владельца от 18:30.
5. НАХОДКА (не делалось): `doctor` не предупреждает о разрешающем дефолте разрешений (о `sandbox_mode` — да);
   claude под root отказывается от `bypassPermissions` без `IS_SANDBOX=1` — не проверено.
6. Из контракта ушёл `SessionState::RequiresAction` (в пакете не назван; причина выше).

---

## A. Контракт, ядро, фейковый драйвер, тесты ядра

Файлы группы: `crates/aae-contract/**`, `crates/aae-core/**`, `crates/aae-testkit/src/{lib,tests,play,step}.rs`,
`crates/aae-testkit/tests/core/**`, `crates/aae-testkit/tests/chaos/**`. Чужие файлы не трогать. Оценка: −450 / +40
строк.

Документы: решения 116, 120, 114 (`docs/agents/decisions.md`), `docs/agents/rules-code.md`, `rules-tests.md`.

### A1. `aae-contract` (шаг 0)

- `src/driver.rs`: удалить enum `Answer` с комментарием (109-115), `SessionState::RequiresAction` (168), enum
  `RequestKind` (171-175), вариант `Normalized::PermissionRequested` (198-204), метод `Driver::respond` (283-289).
- `src/config/mod.rs:72`: удалить `Watchdog::human_timeout_ms`.
- `src/config/check.rs`: удалить `human_timeout_ms: 3_600_000` (299) и ветку `"human_timeout"` (309) — ключ попадает
  в общую ветку `[watchdog]: unknown key`. Отдельного сообщения «retired key» не добавлять (текст пакета: «становится
  неизвестным»).
- `src/config/schema.rs`: удалить строку `"run.requires_action"` (167) и константу `REQUEST` (87 и её тело), если
  больше нигде не используется.
- `src/tests/config_layers.rs`, тест `the_watchdog_deadlines_come_from_the_orchestrator_file` (241-279): убрать поле
  `human_timeout_ms` из обоих ожиданий и `human_timeout = "4h"` из `all`; последний `fails` (ошибка разбора длительности)
  перевести на живой ключ: `silence = "5 minutes"` → `` `silence`: `5 minutes` ``. Неизвестный ключ уже проверяет
  `stall`. Слова `human_timeout` в файле не остаётся.

### A2. Фейковый драйвер `aae-testkit/src` (шаг 0)

- `lib.rs`: удалить `impl Driver::respond` (320-328), импорт `Answer` (10); в шапке (2) убрать «permission requests»,
  в доке `ScriptedDriver` (34-35) — «and per `respond` (the continuation after a permission request)».
- `play.rs`: у `play` единственный вызов остаётся с `Some(..)` — сделать параметр `first: Normalized`
  (внутри: `let first = (!matches!(steps.first(), Some(Step::NoAck))).then_some(first)`), вызов в `lib.rs:294` — без
  `Some`. Параметр, который всегда один и тот же вариант, — нарушение rules-code.
- `tests.rs`: удалить тест `a_permission_request_continues_after_the_answer` (211-248) и `RequestKind` из импорта (2);
  проверить, что `state(..)`/`SessionState` ещё используются другими тестами.

Проверка шага 0: `cargo check -q -p aae-contract && cargo check -q -p aae-testkit --lib`;
`cargo nextest run -p aae-contract`.

### A3. `aae-core/src/run`

- `events.rs`: `attempt_live` (21) — убрать `"requires_action"`; удалить ветку `Normalized::PermissionRequested` (215-248);
  удалить `fn answer` (302-341); в `on_deadline` удалить ветку `("human", "requires_action")` (365-380); шапку файла (2)
  переписать без «answers to requests»; импорты `Answer`, `answer_json` и всё, что стало неиспользуемым. `pause_step`
  остаётся (пауза между ходами, `turn_settled`).
- `decide.rs`: удалить ветки `("requires_action", None)` и `("requires_action", Some(Decision::Answer(..)))` (104-113) и
  две ветки `"run.requires_action"` в `parse` (227-228); импорт `answer_from` (4).
- `hooks.rs`: `DECIDING` — убрать `"run.requires_action"`, размер 4 (26-32); в `decision` убрать ветку
  `"run.requires_action" => Decision::Answer(..)` (365); импорт `driver::Answer` (9).
- `msg.rs`: удалить `Msg::Answer` (47-52), `Decision::Answer` (249-250), `Answer` из импорта (5).
- `mod.rs`: удалить ветку `Msg::Answer` (144), `fn answer_json` (240-247), `pub fn answer_from` (249-261), `Answer` из
  импорта (33).
- `exec.rs`: удалить ветку `(Some("respond"), Ok(handle))` (144-150) и импорт `answer_from` (3).
- `read.rs`: из `run()` убрать `request` (кортеж, SQL-колонка, фильтр 53, `view["request"]` 71) и слова о запросе в
  доке (34); в подсчёте `active` (124) убрать `'requires_action'`.
- `recover.rs`: убрать `'requires_action'` из списка состояний (70), `'respond'` из списка операций (91), условие
  `AND r.state != 'requires_action'` (136); комментарий 107-108 — без `` (`run.requires_action`) `` (сам `DELETE`
  остаётся: решающий хук прежнего события может ещё стоять в очереди).
- `machine.rs:315`: `["state", "silence"]`.
- `../store.rs`: удалить колонку `request TEXT,` у `attempts` (96; `idempotency.request` на строке 19 — другое, не
  трогать); комментарий к `paused_ms` (98) — «when its turn ended: the step timeout pauses between turns»;
  `VERSION` 22 → 23 (решение 114: схема одна, миграции нет).

После правок: `git grep -nE 'requires_action|human|respond|Answer|request' crates/aae-core/src/run` — остаться должны
только несвязанные вхождения (`service_proc.rs`, `writer.rs` и т. п.).

### A4. Тесты ядра `aae-testkit/tests`

- `core/requests.rs`: удалить файл (`git rm -q`), убрать `mod requests;` из `core/main.rs:34`. Первый случай его
  третьего теста (сбой до ack в `starting` → `interrupted`, `quota_exhausted`; инвариант 5) больше нигде не покрыт —
  перенести в `core/driver_events.rs` как `a_failure_before_the_ack_ends_the_attempt_at_once` (сценарий
  `[Step::NoAck, Step::Emit(Normalized::Failure(quota))]`, проверка хвоста состояний `["starting", "interrupted",
  "failed"]` и `failure.kind`).
- `core/watchdog.rs`: из `WATCHDOG` (58-59) убрать `human_timeout = "4m"`, комментарий — «Six»; из
  `every_deadline_comes_from_the_config` удалить блок с запросом (96-108); удалить тест
  `the_step_timeout_pauses_while_a_request_waits_for_its_answer` (159-190); шапка (1-3) и импорты (`Answer`,
  `RequestKind`, возможно `Value`).
- `core/hooks.rs`: удалить тест `a_hook_answers_a_permission_request` (190-216) и ставшие лишними импорты.
- `core/deciding.rs`, тест `a_deciding_event_waits_for_the_slow_hook_of_the_run_queued_ahead`: логика «решающие хуки
  впереди» (`decide.rs:39-47`, `ahead`) остаётся живой, её единственный тест стоял на `run.requires_action`.
  Переписать на оставшийся путь: решающий хук хода без исхода ещё работает, когда попытка падает.
  - `plugin(&[HOLLOW, "run.attempt_failed"], "")`; `Slow` решает `Fail{reason: "judged"}` на оба типа события;
  - сценарий: `hollow_turn()` + `Step::Wait(1 s)` + `Step::Exit{signal: Some(9)}`;
  - ожидание прежнее: `run.failed.reason == "judged"` (без `ahead` дедлайн истёк бы раньше второго хука → `no_decision`);
  - чтобы тест различал и при добавке `GRACE_MS` (10 s) к дедлайну `run.attempt_failed`: timeout хука в `plugin()` —
    `30s`, сон `Slow` — 29 s (второй хук решает на 58-68 s; без `ahead` дедлайн ≤ 46 s, с ним ≥ 66 s), окно
    `w.events` — 120 s. Второй тест файла (`late_session_events...`) с этими числами укладывается в свои 60 s.
  - Обязательная проверка мутацией: заменить `ahead.unwrap_or(tx.now_ms())` на `tx.now_ms()` — тест красный; вернуть.
  - Не выходит воспроизвести (попытка не падает, пока run ждёт решения) — остановиться и описать в отчёте, тест не
    выдумывать.
- `chaos/check.rs`: в `watched` убрать ветку `("requires_action", _) => "human"` (300) и слова «a human deadline while
  it asks» (288); в `undone` убрать `"respond"` (338).

### A. Готово, когда

- `cargo clippy -p aae-contract -p aae-core -p aae-testkit --all-targets -q --message-format short -- -D warnings`
  чисто;
- `cargo nextest run -p aae-contract -p aae-core -p aae-testkit -E 'not binary(chaos)'` зелёный;
- `AAE_CHAOS_SEEDS=96 cargo nextest run -p aae-testkit --test chaos` зелёный (менялась машина прогона);
- `git grep -nE 'requires_action|human_timeout|PermissionRequested|RequestKind|RequiresAction' -- crates/aae-contract
  crates/aae-core crates/aae-testkit` пуст;
- файлы ≤ 400 строк; в отчёте — что удалено, результат мутации в `deciding.rs`.

---

## B. Адаптеры claude и codex

Файлы группы: `crates/aae-claude/**`, `crates/aae-codex/**`, `crates/aae-acceptance/**` (правок не ожидается),
`crates/aae/tests/e2e/{claude,codex,worktree}.rs`, при нужде `crates/aae-testkit/src/{anthropic,openai,mcp}.rs`
(не ожидается). Не трогать: `crates/aae/tests/e2e/replies.rs` (группа C), `crates/aae/tests/common/`. Оценка: −170 / +110.

Документы: решения 116, 117, 38, 90, 61, 4; `docs/agents/rules-code.md`, `rules-tests.md`; `docs/harness-updates.md`
(«чинить адаптер, а не ожидание сценария»).

Ждёт от A (шаг 0, уже сделан к старту): из `aae_contract::driver` исчезли `Answer`, `RequestKind`,
`Normalized::PermissionRequested`, `SessionState::RequiresAction`, `Driver::respond` — см. таблицу в разделе 0.

### B1. claude

- `src/part.rs`, `args` (157-168): `"--permission-prompt-tool", "stdio"` → `"--permission-prompts", "none"`.
- `src/part.rs`, `settings` (96-100): режим по умолчанию. Если в `native.claude` нет ни `permission_mode`, ни
  `permissions.defaultMode` — вернуть `Some("bypassPermissions")`; константа с комментарием «decision 117». Дефолт живёт
  в `process()` (значение `permission_mode` части процесса, входит в отпечаток), `args()` не меняется.
- `src/stream.rs`: удалить ветку `control_request`/`can_use_tool` (136-150), строку `Some("requires_action") => ..`
  (113), `RequestKind` из импорта.
- `src/lib.rs`:
  - удалить поле `Instance::requests` (53-54), его инициализацию и клон (104-109), запись в читателе (151-154),
    `impl Driver::respond` (281-306), `Answer` из импорта;
  - в цикле читателя (после `live.send`): если `msg["type"] == "control_request"` — сразу записать в stdin
    `{"type":"control_response","response":{"subtype":"error","request_id":<msg.request_id>,"error":"aae answers no
    control request"}}`; в ядро ничего не слать (`Stream::line` на него ничего не даёт). Наблюдатель строку видит
    через `live` — так и остаётся (решение 61). Комментарий «почему»: неотвеченный запрос (например `elicitation`
    MCP-сервера) держал бы ход до `stall`. Образец — `crates/aae-codex/src/rpc.rs:171-174`;
  - запись в stdin сейчас в `Claude::write` (76-84) по `InstanceHandle`; читателю нужен тот же код по
    `Arc<Instance>`: вынести тело в `impl Instance { async fn write(&self, line: &Value) -> Result<(), Failure> }`,
    `Claude::write` — в одну строку через него; читателю отдать клон `Arc<Instance>` (сейчас `instance` целиком
    уходит в карту на строке 129). `stop_instance` забирает stdin через `take()`, так что клон у читателя EOF не
    задерживает.
- `src/stream/tests.rs`, `a_permission_request_and_an_interrupted_turn` (81-100): оставить половину про прерванный
  ход, переименовать (`an_interrupted_turn_is_cancelled`), импорт `RequestKind` убрать.
- `src/tests.rs`:
  - ловушка пакета: `process()` на строке 10-13 собирает часть процесса руками с `"permission_mode": null`, поэтому
    `args()` в `every_flag_goes_again...` дефолта не покажет — это верно, дефолт ставит `part::process`. В этот тест
    добавить только проверку пары `--permission-prompts none` и отсутствия `--permission-prompt-tool`;
  - в `the_native_permission_mode_is_a_flag_and_not_a_settings_key` (60-90, идёт через `part::process`/`split`)
    добавить два случая: без `native` → `p["permission_mode"] == "bypassPermissions"` и флаг в `args`; с
    `native = { permissions = { defaultMode = "plan" } }` → `permission_mode` null, флага нет, `settings.permissions`
    на месте;
  - новый тест `a_control_request_gets_an_error_at_once` по образцу
    `a_process_that_dies_in_a_turn_reports_its_stderr_and_is_forgotten` (282-336): скрипт-«claude» печатает
    `{"type":"control_request","request_id":"r1","request":{"subtype":"elicitation"}}`, читает одну строку stdin,
    пишет её в файл и выходит; тест ждёт `Exited`, проверяет в файле `"subtype":"error"` и `"request_id":"r1"` и что
    до `Exited` не пришло ни одного события сессии. Реальный процесс → без `start_paused`.
- `tests/harness/contract.rs`, s13 (333-355): переписать на месте в
  `s13_a_request_the_config_does_not_allow_is_denied_by_the_harness_and_the_turn_completes`:
  `Live::start(plan, dir, 0, Some("manual"))`, план `[Reply::Bash("touch denied"), Reply::Text("OK")]`; ожидание:
  `live.ended() == Completed`, `live.failures()` пуст, файла `rig/denied` нет. Импорт `Answer` убрать. Это первый
  прогон после сборки — он же эксперимент «claude сам отклоняет и продолжает ход». Если claude ведёт себя иначе
  (ход падает, ждёт) — не подгонять ожидание, а остановиться и описать увиденное (`live.seen`).
- Остальные сценарии с `permission: None` (s01-s09, s12, s15) теперь идут в `bypassPermissions` — инструментов там
  нет, ожидания не меняются.

### B2. codex

- `src/part.rs` (131-134): `params["approvalPolicy"]` = значение `native.codex.approval_policy`, иначе `"never"`;
  константа рядом с `DEFAULT_SANDBOX` (19) с комментарием «decision 117». Хост не проверять (см. запись 3 в журнале).
- `src/stream.rs`: удалить ветку `item/*/requestApproval | item/tool/requestUserInput` (192-212); в
  `thread/status/changed` (135-138) `active` → `SessionState::Running` без разбора `activeFlags`; `RequestKind` из
  импорта.
- `src/rpc.rs` (165-175): на любой серверный запрос (`msg.get("id").is_some()`) отвечать сразу:
  `item/commandExecution/requestApproval` и `item/fileChange/requestApproval` → `{"id", "result": {"decision":
  "decline"}}` (ход продолжается); всё остальное (`item/permissions/requestApproval`, `item/tool/requestUserInput`,
  `mcpServer/elicitation/request`, прочие) → прежняя ошибка `-32601`. Ответ собирает чистая функция
  `fn refusal(msg: &Value) -> Value` (её и тестировать). Удалить поле `Instance::requests` (36-37) и его
  инициализацию (`lib.rs:98`).
- `src/lib.rs`: удалить `impl Driver::respond` (287-303), `fn reply` (369-385), `Answer` из импорта.
- `src/tests.rs`: тест `an_approval_is_a_permission_request_and_an_exit_crashes_open_turns` (271-312) — оставить
  половину про выход (`an_exit_crashes_open_turns`: `turn/started`, затем `exited`); тест
  `a_text_answers_every_question...` (316-325) заменить тестом `refusal`: commandExecution и fileChange → `decline`,
  `item/permissions/requestApproval`, `item/tool/requestUserInput`, `mcpServer/elicitation/request` → `error`;
  импорты `RequestKind`, `Answer`.
- `src/part/tests.rs` (рядом с 54): случай без `approval_policy` в `native` → `approvalPolicy == "never"`, случай
  `"untrusted"` → `"untrusted"`.
- `tests/harness/contract.rs`, s13 (148-168): на месте, то же имя, что у claude; `Live::start(plan, dir, 0,
  "untrusted")`, план `[Reply::Command("touch denied"), Reply::Text("OK")]`; ожидание: `Completed`, сбоев нет, файла
  нет. Импорт `Answer` убрать. Поведение иное — описать, не подгонять.

### B3. e2e через демон (проверка дефолта и ловушки `aae finish`)

`aae finish` — Bash-вызов агента: при разрешающем дефолте он должен проходить без единой настройки. Убрать явные
настройки, чтобы существующие тесты проверяли именно дефолт:

- `crates/aae/tests/e2e/claude.rs:34` и `worktree.rs:54` — убрать `[native.claude]\npermission_mode =
  \"bypassPermissions\"\n`;
- `crates/aae/tests/e2e/codex.rs:35` — убрать `approval_policy = \"never\"\n` (остальной `[native.codex]` оставить).

Сценарии `aae-acceptance` (`scenarios/mod.rs:280,323,349`, `c04a`, `c17`) не трогать: их локально не запускают.
Запустить e2e можно только после A и C — это делает coder в разделе Z.

### B. Готово, когда

- `cargo clippy -p aae-claude -p aae-codex --all-targets -q --message-format short -- -D warnings` чисто;
- `cargo nextest run -p aae-claude -p aae-codex` зелёный (контрактные сценарии идут на настоящих бинарях против
  фейкового провайдера; предел времени на команду обязателен), оба s13 и `a_control_request_gets_an_error_at_once`
  в их числе;
- `git grep -nE 'requires_action|PermissionRequested|RequestKind|RequiresAction|permission-prompt-tool|fn respond' --
  crates/aae-claude crates/aae-codex` пуст;
- число тестов в модулях `contract::` и `models::` не изменилось (иначе поправить 41 в
  `crates/aae-acceptance/src/checks.rs:76`); файлы ≤ 400 строк (`contract.rs` claude сейчас ровно 400 — не расти);
- `TESTED` не менять (расширение диапазона требует живого хода, это решает владелец).

---

## C. Фронт, OpenAPI, документы, демо

Файлы группы: `crates/aae-front/**`, `crates/aae/tests/e2e/replies.rs`, `docs/**` (кроме `docs/agents/decisions.md`),
`README.md`, `demos/*/root/config.toml`, `demos/1-planner-executor/README.md`. Оценка: код −75, `openapi.json` −90
(генерируется), документы около −70 / +60, демо −9.

Документы: решения 116, 117, 120, 90, 38, 61; `CLAUDE.md` («docs описывает, как продукт работает сейчас: истории
нет»; `docs/` — по-английски); `docs/agents/rules-tests.md` раздел «The demos» (демо не запускать).

Ждёт от A: из `aae_core::run` исчезли `Msg::Answer`, `answer_from`; у вида прогона нет поля `request`; состояния
`requires_action` нет. Правки фронта от порядка не зависят (удаляются только использования), но собрать фронт можно,
когда `cargo check -q -p aae-core` зелёный.

### C1. `aae-front`

- `src/api/runs.rs`: удалить `struct AnswerRun` и обработчик `answer` (158-188), `answer_from` из импорта (7),
  `"requires_action"` из `STATES` (79; размер массива 10), «answer a request» из шапки (1).
- `src/api/mod.rs:93`: удалить `.routes(routes!(runs::answer))`; `AnswerRun` из списка схем, если он там назван.
- `src/api/replies.rs:51-54`: удалить поле `request` у ответа прогона.
- `src/cli/commands.rs`: удалить `RunCommand::Answer` (71-80), enum `Decision` (84 и тело), ветку исполнения (227-246).
- `src/cli/mod.rs`: `Decision` из `pub use` (12), строку `("run answer", ..)` из `OPERATIONS` (26).
- `docs/api/openapi.json`: не править руками — `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`, затем
  `cargo nextest run -p aae-front` (тест сверяет файл с кодом и «CLI покрывает ровно операции API»).
- `crates/aae/tests/e2e/replies.rs`, `the_errors_answered_are_declared_with_their_codes` (100-180): удалить `answer`,
  `answer_checked` и два случая `/answer` (127-142). Это был единственный случай 409 `rejected` — заменить одним:
  отмена уже завершённого прогона `checked` (`POST /v1/runs/{id}/cancel`, `{}` → 409; дождаться его терминального
  состояния имеющимся помощником `tests/common`, не сном). Число тестов модуля не менять. Запуск — в разделе Z.

### C2. Документы (английский; только нынешнее поведение, без «было»)

Найти всё: `git grep -nE 'requires_action|human_timeout|run answer|/answer|permission\.requested|respond|human answer|permission request|permission-prompt-tool' -- docs README.md ':!docs/api/openapi.json' ':!docs/agents/decisions.md'`.

- `docs/design/contracts.md`:
  - §1.3: поле `request` вида прогона (76), строки `run.requires_action` в таблицах решающих событий (129) и
    полезных нагрузок (139) — удалить;
  - §3.2 (247), §3.3 (265-269, 281), §3.4 (300): убрать `run.requires_action`, ожидание человека и `human_timeout`;
  - §4.1 (334-335, 355): убрать `respond` из интерфейса и «requires_action (4.3)» из описания; §4.2 (375): строку
    `Answer`; §4.3 (397-398): `state: running | idle`, строку `permission.requested` удалить; (407) — без «Waiting for
    a human»;
  - §4.6 claude (487, 505-510) — заменить три пункта одним: claude runs with `--permission-prompts none`: whatever
    would prompt is denied by claude itself and the model sees the denial, the permission mode decides the rest;
    the mode is `bypassPermissions` unless `native.claude` sets `permission_mode` or `permissions.defaultMode`; any
    `control_request` of the CLI gets an error `control_response` at once, so nothing waits. Наблюдение про heredoc
    под `bypassPermissions` (2.1.325) оставить как «denied at once»;
  - §4.6 codex: добавить пункт: the thread starts with `approvalPolicy = "never"` unless `native.codex.approval_policy`
    sets another; under another policy the adapter declines command and file-change approvals
    (`{"decision":"decline"}`, the turn goes on) and answers every other server request with an error;
  - §6.1 (621, 627, 669): строку состояния `requires_action` и `human_timeout` удалить, «pauses like in
    `requires_action`» переформулировать (пауза между ходами описывается сама); §6.2 (690-691, 706): две строки
    переходов удалить, из списка живых состояний убрать; §6.4 (779); §10 (944).
- `docs/design/invariants.md`: инвариант 15 (32) — без «Waiting for a human is not silence»; восстановление (63) — без
  `requires_action`.
- `docs/design/config.md`: (300) удалить строку `human_timeout`; (34-40, 52) дописать дефолты и политику — см. C3.
- `docs/design/experiments.md:50`: строка о запросе разрешения — оставить факт эксперимента без литерала состояния и
  сказать, что адаптер этим не пользуется (`--permission-prompts none`).
- `docs/design/adapters.md:64`, `contracts.md:471`: «never the answer to a request» — проверить смысл, поправить, если
  речь об ответе на запрос разрешения.
- `docs/overview.md:70`: «A human gate as a formula step | An operator hold does that job» (без permission request);
  в словаре/таблице «не входит в первый релиз» при необходимости — «answering a permission request by a human: not
  planned, the harness decides (config)».
- `docs/api/frontends.md`: (40) убрать «answer a request»; (104-105) — человек видит прогоны и живой вид, на запросы
  разрешений не отвечает: их решает харнесс по конфигу.
- `docs/guide/first-formula.md`: см. C3 (строки 19-20 и 101-107).
- `docs/guide/plugin.md`: (8) убрать «answer a permission request»; (102) строку `run.requires_action`; (118) фразу про
  хук `run.requires_action`.
- `docs/harness-updates.md`: (36) «a permission request» → «a request the config does not allow»; строка 13 таблицы
  claude (64): `| 13 | a command the permission mode does not allow | claude denies it by itself (`manual` mode,
  `--permission-prompts none`), the command does not run, the turn completes |`; в таблицу отличий codex добавить
  строку 13: policy `untrusted`: the adapter declines the approval, the command does not run, the turn completes;
  последняя фраза (85) — без «and permission».
- `docs/agents/rules-tests.md:69`: из перечня сценариев фейкового драйвера убрать «a permission request».
- `README.md:39`: удалить фразу про `requires_action` в описании `aae drain`.

### C3. Как пользователь задаёт политику (новый текст, коротко)

Место — `docs/design/config.md` рядом с описанием `native.<harness>` (34-52): один абзац или короткий подраздел, из
гайда — ссылка на него. Содержание (решения 116, 117, 120):

- A run is unattended: nothing waits for a human, and every permission is decided by the harness from the config set
  before the start, on any layer (`native.<harness>` of the orchestrator, project, rig, agent or formula).
- Defaults are permissive: claude `--permission-mode bypassPermissions`, codex `approval_policy = "never"` (beside
  `sandbox_mode = "danger-full-access"`); what a stricter config does not allow is denied at once.
- claude: `native.claude.permission_mode`, `permissions.allow` / `deny` / `ask` (an `ask` rule is a denial here), the
  harness hooks `PermissionRequest` and `PreToolUse`.
- codex: `native.codex.approval_policy` (granular form included), `sandbox_mode`, the `PreToolUse` hook.
- A hook may ask aae or another agent for the decision through the CLI or the MCP of aae: the session environment
  carries `AAE_RUN`, `AAE_ATTEMPT_TOKEN` and `AAE_SOCKET`.
- Ловушка: `aae finish` is a shell call of the agent. Под дефолтом проходит; со строгими правилами его надо разрешить
  явно: claude — `permissions.allow = ["Bash(aae finish *)"]`.

`docs/guide/first-formula.md`: из примера `agent.toml` убрать блок `[native.claude] permission_mode = "acceptEdits"`
(19-20) — первый пример идёт на дефолте; абзац 101-107 с `aae run answer` заменить: по умолчанию вызов проходит; если
задан строгий режим — пример

```toml
[native.claude]
permission_mode = "acceptEdits"
permissions = { allow = ["Bash(aae finish *)"] }
```

и фраза, что не разрешённое конфигом отклоняется сразу. Про codex писать только проверенное: при политике, отличной от
`never`, команда, требующая одобрения, отклоняется; готовых правил для `aae finish` не выдумывать.

### C4. Демо (не запускать)

- `demos/1-planner-executor/root/config.toml` и `demos/2-multirepo-qa/root/config.toml`: удалить комментарий и блок
  `[watchdog]` с `human_timeout = "2m"` целиком (строки 7-10; других ключей в блоке нет — проверить).
- `demos/1-planner-executor/README.md:50-52`: убрать фразу про запрос, отклоняемый через две минуты, и
  `human_timeout`; оставить, что режим разрешений — родная настройка харнесса. `aae.toml` демо не трогать.

### C. Готово, когда

- `cargo clippy -p aae-front --all-targets -q --message-format short -- -D warnings` чисто;
  `cargo nextest run -p aae-front` зелёный, `docs/api/openapi.json` перегенерирован и в нём нет `/answer`;
- `git grep -nE 'requires_action|human_timeout|run answer' -- docs README.md demos crates/aae-front
  ':!docs/agents/decisions.md'` пуст;
- относительные ссылки и якоря в изменённых документах целы (удалённые разделы никто не ссылает:
  `git grep -n 'requires_action\|#humans-and-runs'`); строки `docs/` ≤ 120 знаков, как в соседних абзацах.

---

## Z. Интеграция (coder, после трёх групп)

1. `cargo build -q --workspace --message-format short 2>&1 | head -60`;
   `cargo clippy --workspace --all-targets -q --message-format short -- -D warnings 2>&1 | head -60`;
   `cargo fmt --all --check`.
2. Точечно то, что группы не могли запустить:
   `cargo nextest run -p aae -E 'test(/^claude::|^codex::|^worktree::|^replies::/)'` (дефолт разрешений через демон,
   `aae finish` без настроек, API без `/answer`).
3. Критерий пакета: `git grep -nE 'requires_action|human_timeout|run answer' -- . ':!.loop'` — остаются только
   `docs/agents/decisions.md:40` и `experiments/*/logs/*.ndjson` (запись 1 в журнал).
4. `scripts/unused-pub.sh`, `scripts/budget.sh` (после удалений могли осиротеть `pub`-элементы).
5. Записи в журнал из раздела 0; `.loop/PLAN.md`: статус WP-94. Коммит(ы) — по группам путей A / B / C либо один,
   сообщение по-английски; затем `scripts/loop/checks 109`.

## Риски

- Поведение claude в `manual` + `--permission-prompts none` и codex в `untrusted` + `decline` живьём не снималось:
  первый прогон обоих s13 — эксперимент. Иной результат — стоп и разбор, ожидание не подгоняется.
- `bypassPermissions` по умолчанию: claude под root и при управляемой настройке, запрещающей обход, может отказаться
  стартовать — не проверено, в журнал как находка.
- Тест `deciding.rs` опирается на то, что попытка может упасть, пока run ждёт решения по ходу без исхода; если путь
  недостижим, логика `ahead` осталась без теста — это надо сказать в отчёте, а не закрыть молча.
- Параллельная работа в одном дереве: до конца шага 0 никто, кроме его исполнителя, cargo не запускает.

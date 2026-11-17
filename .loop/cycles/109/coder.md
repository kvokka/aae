# Цикл 109 — coder
STATUS: done

## Взято в работу
- Обязательные пункты 1–2 ревью 108 (доделка WP-93) и Желательное 1, 5.
- WP-94 (M, критичный): план expert → реализация тремя исполнителями.
- WP-95 (S): запас контекста остался — взят следующим, один subcoder.

## Сделано (коммиты)
- 977d29b WP-93: дедлайн решения `run.attempt_failed` учитывает ожидание хуками остановленного процесса
  (`STOP_GRACE`, 10 s) — и для своего события (`deciding`), и для стоящих впереди эффектов и пересчёта после
  рестарта (`decide::queued()`, общий SQL-фрагмент для `deciding` и `recover::rearm`). Правка 3.3 в contracts.
  Тесты в `crates/aae-testkit/tests/core/attempt_edges.rs`:
  - `a_decision_on_a_failed_attempt_waits_for_the_stopped_process` (+ вариант `_after_a_restart`): процесс живёт
    дольше grace, хук с timeout 1 s решает сразу после ожидания — решение плагина принято;
  - `a_resume_after_a_new_session_that_never_started_leaves_the_continued_session` (Обяз. 2);
  - `a_new_session_after_a_daemon_crash_gets_the_step_prompt_once` (Жел. 1).
  Мутации проверены: убрать `grace` из `deciding` → красный первый; убрать grace из `queued()` → красный вариант с
  рестартом; убрать `OR kind = 'new_session'` → красный тест continue_from; `n > 1` → `n >= 1` → красный тест
  промпта шага. Попутно перенесён по ширине абзац contracts.md про watchdog харнессов (Жел. 5).

- 0e3bd74 WP-94: разрешения решает харнесс, ничего не ждёт человека (64 файла, +430/−1023). План — expert
  (`notes/wp94-plan.md`), реализация — три subcoder на непересекающихся файлах, сводки `notes/wp94-{A,B,C}.md`.
  - claude: `--permission-prompts none`, `bypassPermissions`, если ни `permission_mode`, ни
    `permissions.defaultMode` не заданы; любой `control_request` сразу получает `control_response{error}`, в ядро
    ничего не идёт. Тест `a_control_request_gets_an_error_at_once`.
  - codex: `approvalPolicy = "never"` по умолчанию; при чужой политике `decline` на commandExecution/fileChange
    approval, JSON-RPC-ошибка на остальные серверные запросы (журнал, РЕШЕНИЕ).
  - Ушли: `requires_action`, `run.requires_action`, `human_timeout` (теперь неизвестный ключ), `Msg::Answer`,
    `Decision::Answer`, эффект `respond`, `Driver::respond`, `PermissionRequested`, `RequestKind`, `Answer`,
    `SessionState::RequiresAction`, `attempts.request` (схема 23, без миграции — по правилу `store.rs`), API
    `/answer`, `aae run answer`; openapi перегенерирован. `core/requests.rs` удалён, проверка «сбой до ack»
    перенесена в `driver_events.rs`; `deciding.rs` переписан (мутация `ahead` → красный).
  - s13 обоих адаптеров переписаны и зелёные на установленных CLI с фейковым провайдером (claude `manual` сам
    отклоняет Bash и завершает ход; codex `untrusted` → `decline` → ход завершён).
  - Документы: новый раздел `### Permissions` в `docs/design/config.md` (как задать свою политику средствами
    харнесса), правки contracts/invariants/overview/frontends/guide/README; `human_timeout` убран из двух
    `demos/*/root/config.toml` и README демо 1 (разрешено владельцем; демо не запускались).
- 4036b25 WP-95: событие `run.permission_denied` (13 файлов, +204/−14; сводка `notes/wp95.md`).
  `Normalized::PermissionDenied {tool, tool_use_id, input, reason}`; ядро публикует в `starting` и `running`,
  ничего не ждёт. claude: оба источника, одно событие на `tool_use_id` за ход (набор очищается на `result`);
  codex: каждый отказанный адаптером серверный запрос. `aae run watch` фильтрует только по прогону — событие
  видно без правок. Тесты: `aae-claude/src/stream/tests.rs`, `aae-codex/src/tests.rs`,
  `aae-testkit/tests/core/driver_events.rs`. Ограничение (журнал, НАХОДКА): поля берутся из первого источника —
  при system-пути `input` = null.
- Гейт: `scripts/loop/checks 109` на 4036b25 — pass, 31 s, nextest 544/544.

## Не получилось и почему
- Критерий grep WP-94 выполнен с исключением `docs/agents/decisions.md:40` (текст решения 116) и `experiments/*/logs`
  — записано в журнал.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего. Следующий пакет по плану — WP-96.
- Риски/непроверенное (журнал, НАХОДКА): claude под root и `bypassPermissions`; `doctor` не предупреждает о
  разрешающем дефолте. Вопрос владельцу — дефолт не отменяется настройкой хоста при `inherit_host`.
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1, Обяз. 2 — закрыты (977d29b). Жел. 1, Жел. 5 — закрыты.
- Жел. 2 (ожидание 200 ms в `no_prompt_goes_into_a_session_a_stopped_writer_never_recorded`) — не тронуто.
- Жел. 3–4 — отступления от критериев WP-92: тест `silence_after_the_prompt_is_a_stall_at_its_budget` проверяет
  дефолт сквозь ядро, а `[watchdog]` не выводит ни одна команда CLI — критерий пакета про `config explain` был
  неточен, дефолты проверены тестами `aae-contract` и ядра.
## Контекст на момент отчёта: ~125k

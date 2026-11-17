# WP-94 · группа B (адаптеры claude и codex) — сводка subcoder

## Сделано

B1 claude:
- `part.rs`: `--permission-prompt-tool stdio` → `--permission-prompts none`; константа `DEFAULT_PERMISSION_MODE =
  "bypassPermissions"` (decision 117) ставится в `settings()`, если в native нет ни `permission_mode`, ни
  `permissions.defaultMode`; значение идёт в `permission_mode` части процесса (в отпечатке), `args()` без изменений.
- `stream.rs`: убраны ветка `control_request/can_use_tool`, `requires_action`, импорт `RequestKind`.
- `lib.rs`: убраны `Instance::requests`, `impl respond`, `Answer`; запись в stdin вынесена в `Instance::write`
  (`Claude::write` — через неё); читатель держит клон `Arc<Instance>` и на любой `control_request` сразу пишет
  `control_response{subtype:"error", request_id, error:"aae answers no control request"}`; в ядро ничего, в `live` строка
  уходит как раньше.
- `stream/tests.rs`: тест переименован в `an_interrupted_turn_is_cancelled`.
- `tests.rs`: проверка `--permission-prompts none` (и что флаг `--permission-prompt*` ровно один), два случая дефолта в
  `the_native_permission_mode_is_a_flag...`, новый `a_control_request_gets_an_error_at_once`. Чтобы уложиться в 400
  строк, введён хелпер `config(native)` для двух тестов (394 строки).
- `tests/harness/contract.rs`: s13 переписан на месте → `s13_a_request_the_config_does_not_allow_is_denied_by_the_harness_and_the_turn_completes`
  (`Some("manual")`, `Bash("touch denied")`, Completed, без сбоев, файла нет). 389 строк.

B2 codex:
- `part.rs`: `approvalPolicy` = native `approval_policy`, иначе `DEFAULT_APPROVAL = "never"` (decision 117).
- `stream.rs`: ветка `item/*/requestApproval | requestUserInput` удалена; `active` → `Running` без `activeFlags`.
- `rpc.rs`: поле `requests` удалено; на любой серверный запрос сразу `refusal(&msg)`: `decline` для
  `item/commandExecution/requestApproval` и `item/fileChange/requestApproval`, ошибка `-32601` для остальных.
- `lib.rs`: убраны `respond`, `fn reply`, `Answer`, инициализация `requests`.
- `tests.rs`: `an_exit_crashes_open_turns`; тест `reply` заменён на `every_server_request_is_refused_at_once`.
- `part/tests.rs`: без `approval_policy` → `"never"`, `"untrusted"` → `"untrusted"`.
- `tests/harness/contract.rs`: s13 на месте, то же имя, что у claude (`"untrusted"`, `Command("touch denied")`).

B3 e2e: из `crates/aae/tests/e2e/{claude,worktree}.rs` убран `[native.claude] permission_mode`, из `codex.rs` —
`approval_policy = "never"`.

## Расхождения с планом

- План просил в тесте проверить «отсутствие `--permission-prompt-tool`», но критерий grep требует, чтобы этой строки в
  крейте не было. Проверка сделана без литерала: аргументов, начинающихся на `--permission-prompt`, ровно один.
- Хелпер `config(native)` в `aae-claude/src/tests.rs` — вынужденный (иначе 406 строк > 400).

## Проверка

- `cargo clippy -p aae-claude -p aae-codex --all-targets -D warnings` — чисто.
- `cargo nextest run -p aae-claude -p aae-codex` — 75/75 зелёные, в том числе оба s13 на настоящих claude/codex против
  фейкового провайдера и `a_control_request_gets_an_error_at_once`. Поведение харнессов совпало с ожидаемым в плане:
  claude в `manual` с `--permission-prompts none` сам отклоняет Bash и завершает ход; codex в `untrusted` присылает
  approval, адаптер отвечает `decline`, ход завершается.
- grep `requires_action|PermissionRequested|RequestKind|RequiresAction|permission-prompt-tool|fn respond` по обоим
  крейтам пуст. Число тестов в `contract::`/`models::` не изменилось (переписаны на месте). `TESTED` не трогал.
- После `core.done`: `cargo check -p aae --tests` собирается (предупреждение `unused import ValueEnum` в
  `aae-front/src/cli/commands.rs` — файл группы C); e2e `claude::`, `codex::`, `worktree::` — 12/12 зелёные на
  дефолте без явных настроек разрешений (`aae finish` проходит).

## Не делалось

- `aae-testkit/src/{anthropic,openai,mcp}.rs` и `aae-acceptance` не менялись (не понадобилось).
- Полный `mise run ci` не запускался.

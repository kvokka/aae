# Цикл 111 — coder
STATUS: done

## Взято в работу
WP-97 (M) — классификация отказов по структурным полям; WP-98 (S) — сбой старта MCP-сервера проваливает попытку.

## Сделано (коммиты)
- ddb462b — тесты потока codex вынесены в `crates/aae-codex/src/stream/tests.rs` (подготовка, файл был 384 строки).
- 5b209f1 — claude: `KINDS` и склейка убраны; `stream/classify.rs`: порядок `startup_failure_reason` →
  `subtype`/`terminal_reason` → `assistant.error` (13 значений) → `api_error_status` → два узких текста (`Not logged
  in`; `You've hit your` только для rate-limited хода) → `unknown` с текстом (`result`, иначе `errors[]`, иначе
  `поле=значение`). `rejected`/`credits_required` уточняют только `rate_limited`. Env
  `CLAUDE_CODE_STARTUP_FAILURE_RESULTS=1`. Ранний `auth_required` в `api_retry` на 401/403 и
  `authentication_failed`/`oauth_org_not_allowed`/`cloud_credential_error`. Тесты lib claude разбиты
  (`src/tests.rs` 398 → 152 + `src/tests/process.rs`), классификация — `stream/tests/classify.rs`.
- de6c609 — codex: `KINDS` убран; разбор `codexErrorInfo` как `(вариант, httpStatusCode)`; в снапшоте лимитов
  хранятся `rateLimitReachedType` и `spendControlReached` (→ `billing`); ранний `auth_required` по 401/403;
  `rpc.rs` — ветка −32001 → `overloaded`. Тесты — `stream/tests/classify.rs`.
- ae4d34f — документы: contracts 4.6 (порядок полей обоих адаптеров), adapters.md, harness-updates.md.

Дефекты: C1, C2, C3 — строки таблицы `failed_turns_are_classified`; C4 — `an_unknown_failure_keeps_the_words...`;
C8 — строки `startup_failure_reason` (форма взята с живого claude 2.1.326: `TMPDIR=/nonexistent` →
`result{subtype:error_during_execution, startup_failure_reason:temp_dir_unusable, errors:[…]}`, без модели) и
проверка env в `tests/process.rs`; C5, C6 — `a_usage_limit_of_a_plan_or_credit_is_billing`; C7 — строки
`every_final_error_has_its_kind`. Мутации (408, хранение полей снапшота, −32001) краснят тесты.

Версии сверки: `@anthropic-ai/claude-agent-sdk` 0.3.326 (`claudeCodeVersion` 2.1.326), codex tag `rust-v0.168.0`
(`codex-rs/app-server-protocol` v2, `protocol/src/error.rs`). Выписка — `notes/harness-error-fields.md`.
Контрактные сценарии обоих адаптеров зелёные без правки ожиданий. Гейт на ae4d34f — pass, 35 s, 549/549.

Решения по значениям, которых не было в плане (в коде и contracts 4.6): `blocking_limit` → `context_overflow`;
`verification_required` → `auth_required`; `flexUnavailable`, `policyCapacityUnavailable`, 408, 504 →
`overloaded`; `sandboxError` → `config_error`; startup-причины, которые могут пройти сами
(`org_verify_failed`, `remote_settings_required_unavailable`, `org_config_required_unavailable`,
`managed_settings_not_delivered`) → `unknown`; `session_held_by_background` → `transcript_divergence`.

- 9887aae — WP-98: codex ставит `required = true` каждому серверу aae → `thread/start`/`thread/resume` падает
  «required MCP servers failed to initialize» → `tool_failure` до первого промпта (контрактный сценарий s16 на
  реальном codex 0.168 против фейка: без `required` краснеет, к провайдеру не уходит ни одного запроса).
  `mcpServer/startupStatus/updated{failed}` сервера рига/хоста → `tool_failure` треду: в ходе — сразу, иначе
  держится до конца `start_session`/`resume_session`, следующего `send_prompt` или `turn/started` (ядро
  отбрасывает события сессии, которой ещё не знает). claude: `needs-auth` в `init` — тоже `tool_failure`.
  Документы: contracts 4.6, harness-updates.md (16-й сценарий codex). Хелпер `Live::open` в тестах codex принимает
  `tweak` конфига вместо `hooks`.

Гейт на 9887aae — pass, 35 s, 551/551.

## Не получилось и почему
- WP-98: «`mcp_server_errors[]` — в текст отказа» не сделано: такого поля в SDK 0.3.326 нет (журнал).
- WP-98: путь «держать до привязки» через `lib.rs` (`held` в `start_session`/`send_prompt`) проверен только
  юнит-тестом потока; живого сценария с сервером рига в `.codex/config.toml` нет (codex читает его только у
  доверенного проекта).
## Начато и не закончено (файлы, точка остановки, следующий шаг)
## Замечания предыдущего ревью: что закрыто, что нет
Обязательных нет. Жел. 1 — `aae-claude/src/tests.rs` разбит до правки; `items.rs` и `tests/harness/contract.rs`
claude не трогались. Жел. 2–4 не трогались (вне пакета).
## Контекст на момент отчёта: ~180k

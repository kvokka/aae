# Цикл 17 — coder
STATUS: done

## Взято в работу
1. Обязательный 1 ревью 16 (паника писателя на чужой форме `native.claude.hooks`).
2. WP-26b — поддельный OpenAI Responses API.
3. WP-29 — адаптер codex, основной путь на подделке и через демон. Остаток вынесен в новый WP-29b (S).
4. Попутно: желательные 1 и 3 ревью 16.

## Сделано (коммиты)
- `ef11049` WP-27b: `settings()` в `aae-claude/src/part.rs` — `hooks` не объект или `hooks.<event>` не массив →
  `config_error`, без `IndexMut`; юнит на `{hooks = "x"}` и `{hooks = {Stop = {}}}` в `aae-claude/src/tests.rs`.
- `4102097` WP-26b: `aae-testkit/src/openai.rs` — план по номеру запроса: `Text`, `Command` (`exec_command`),
  `RateLimited`, `UsageLimit` (`x-codex-*`, сброс через час), `StrangeQuota`, `InsufficientQuota`, `Overloaded` (503),
  `Unauthorized`, `ContextOverflow` (SSE `response.failed`), `NoModel`, `Hang`, `Silent`. `tests/openai.rs`: каждый
  режим прямым HTTP + настоящий `codex exec` 0.167.2 выполняет команду подделки и заканчивает на её тексте.
- `6b5afce` WP-29: `crates/aae-codex` — `lib.rs` (Driver), `rpc.rs` (JSON-RPC по stdio, серверные запросы, читатель),
  `stream.rs` (нормализация по тредам, классификация по `codexErrorInfo`, квота с `retry_at` из `rateLimits`, 401 —
  `auth_required` с первого повтора, фон по незавершённым `commandExecution`), `part.rs` (процесс: свой
  `CODEX_HOME`+`HOME` в state по отпечатку, `native.codex` → `-c`, ссылка на `auth.json` хоста; тред: cwd, model,
  approvalPolicy, developerInstructions, MCP, hooks+`bypass_hook_trust`, `AAE_RUN`/`AAE_ATTEMPT_TOKEN`).
  `tests/codex.rs` на настоящем codex: команда видит `AAE_RUN`, `PromptAcked`, транскрипт `Answered`/`Absent`, квота.
- `c5dab93` WP-29: демон регистрирует claude и codex из `[harness.*]`; предупреждение о версии строится от
  `capabilities().versions` (`aae-core/src/version.rs`, `untested`), `aae-claude/src/version.rs` удалён (желательный 1).
  `idle` треда codex шлётся после `TurnEnded` (ядро закрывает ход на нём). `crates/aae/tests/codex.rs`: настоящий
  демон + codex → `aae finish pass` → `passed`.
- `08aed80` план (WP-26b, WP-29 done, новый WP-29b, WP-30 зависит от него), `READINESS.md` (codex 4, реестр),
  `docs/design/config.md` (`native.codex`, `permission_mode` у codex).
- `5397ad8` `experiments.md`: при наследовании личные MCP хоста идут в сессию (желательный 3).
- `mise run ci` зелёный: 156 тестов, deny/machete/budget ok; codex 966/3500, всего 8583/30000.

## Не получилось и почему
- Субагентов нет (только `coder`/`reviewer`), всё в одном контексте; WP-28 не взят.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего незакоммиченного. Остаток codex — WP-29b в плане: skills, `max_sessions`, resume после рестарта демона,
  форма ответа `requestUserInput` (`shortcut:` в `aae-codex/src/lib.rs`), реакция codex на `insufficient_quota`
  и `model_not_found` подделки.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательный 1 — закрыт (`ef11049`).
- Желательный 1 — закрыт: `Capabilities.versions` читает демон.
- Желательный 2 (`aae-claude/src/lib.rs` 396 строк) — не сделан, остаётся на WP-28.
- Желательный 3 — закрыт (`5397ad8`).
- Желательные 4, 5 — в пакетах (WP-28, WP-20/21/22b/33), не трогались. 6 — харнесс, запись в журнале.

## Контекст на момент отчёта: ~232k

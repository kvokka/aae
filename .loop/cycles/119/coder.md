# Цикл 119 — coder
STATUS: done

## Взято в работу

WP-109 (M, критичный): окружение exec-шагов — `[exec]`, `inherit_env`, `env` со ссылками `${NAME}` (решение 137).
План до кода — expert: `.loop/cycles/119/notes/wp109-plan.md`.

## Сделано (коммиты)

`b5c89c5` WP-109: exec steps take [exec], inherit_env and env with ${NAME} references.
- `aae-contract/src/config/env.rs` (новый): `env_value` (строгий разбор `${NAME}`/`$$`, любой другой `$` — ошибка),
  `env_literal`, `ExecFile`/`ExecEnv` (слияние слоёв `under`, `explain`), `Loader::env_values` — проверка формы при
  загрузке во всех `[settings] env`, `[exec] env`, `env` exec-шага.
- Слои: `[exec]` в `OrchestratorFile`/`ProjectFile`, ключи `inherit_env`/`env` у exec-шага; шаг → проект → оркестратор
  через `formula_defaults`; `Project.exec` для check-скриптов шагов не-exec. `Loader::text` перенесён в `prompt.rs`
  (items.rs у потолка 400, сейчас 397).
- Литералы aae экранируются: каталог плагина в `env`, окружение трекера, `PATH` с `bin/` плагинов, `env` хука
  `run.preparing`.
- Ядро: снимок и эффект хранят шаблоны (`env`, `inherit_env`, отдельно `context` с `AAE_*` и актором); разрешение —
  только в `exec_step::run` при спавне; нет переменной → `AttemptFailed{config_error}` у шага, `SpawnFailed` →
  `infrastructure` у check-скрипта. Check-скрипт ai-шага получает `[exec]`, а не `env` агента (подключ `exec` в снимке).
- Адаптеры: `child::base_env` → `Result`, разрешает ссылки, `config_error` с ключом и переменной; часть процесса,
  отпечаток и `account` остаются со ссылкой. `config explain --formula` печатает `exec.*` строки.
- Документы: config.md («Environment values», Effective config, Orchestrator, Project, Formula, Plugin), contracts
  3.2/4.4/4.5/6.2, adapters.md, guide/plugin.md, assumptions.md.
- Тесты (subcoder ×2): `aae-contract/src/tests/config_env.rs` (8), `driver/child/tests.rs` (1), адаптеры (по 1),
  `aae-testkit/tests/core/exec_env.rs` (7), `step_repeat_more.rs` (+2), e2e `exec_env.rs` — маркер секрета из окружения
  демона не встречается ни в одном файле каталога состояния (state.db+WAL, журнал, лог), ни в `run show` (текст и
  json), ни в `config explain`. Мутации (11 штук: разбор `$$`/`$`, `under`, `in_plugin`, `base_env`, разрешение в
  исполнителе/в `command()`, `inherit_env`, порядок `context`, `env_literal` в decide, окружение check-скрипта) —
  каждая краснит хотя бы один тест.
- Гейт: `scripts/loop/checks 119` — pass на `b5c89c5`, 43 s, nextest 614/614.

## Не получилось и почему

Ничего. Отступление в D2: «`AAE_RUN` поверх унаследованного» напрямую не проверить без `set_var` (запрещён); проверено
`env` поверх унаследованного и `AAE_RUN` поверх `env`.

## Начато и не закончено (файлы, точка остановки, следующий шаг)

Нет. Следующий по плану — WP-110 (демо 3 на `gh` хоста, зависит от WP-109).

## Замечания предыдущего ревью: что закрыто, что нет

«Обязательно» в ревью 118 — нет. Жел. 1, 3, 5 (`run.sh` демо 3) — по плану в WP-110; Жел. 2 — WP-113; Жел. 4 закроет
новый прогон в WP-110. Жел. 6 — вне плана.

Попутно: chaos `a_seed_replays_the_same` один раз упал в общем прогоне под нагрузкой (известный флейк, WP-112),
отдельно 3/3 зелёный.

Журнал: РЕШЕНИЕ (допущения кодера по WP-109) и ВОПРОС (ссылки в `env` MCP, `[[service]]`, хуков — сейчас литералы).

## Контекст на момент отчёта: ~125k

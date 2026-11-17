# Цикл 113 — coder
STATUS: done

## Взято в работу

- WP-101 (S): скилл восстановления как плагин Claude Code и Codex.
- Жел. 1, 2, 3 ревью 112.

## Сделано (коммиты)

- `d6a225f` WP-99 (Жел. 1 и 2 ревью 112). Вне хода, пока жив `local_agent`, адаптер claude после события субагента
  (`Retrying`, `Usage` из `rate_limit_event`, `PermissionDenied`) отдаёт ещё `State{Idle, background:true}`. Ядро
  снова взводит `background_silence`, а не `silence`. Данные не теряются: `run.retrying`/usage/denial доходят как
  раньше. `stop_background` дедуплицируется ключом `stop_background:{attempt}`. Правка в
  `crates/aae-claude/src/stream.rs` (`line`), строка в contracts 4.6. Тест
  `a_background_subagent_keeps_the_session_idle_with_background…` дополнен:
  - после `result` при одном `local_bash` нет `State`, поэтому мутация `!tasks.is_empty()` теперь краснит тест;
  - api_retry, rate_limit_event и permission_denied вне хода заканчиваются на `idle(true)`.
- `f73ab51` WP-101:
  - `harness-plugin/`: `.claude-plugin/plugin.json` (`aae`, версия = версия workspace) и
    `skills/recover/SKILL.md`. Скилл находит упавшие прогоны, показывает class/reason/попытки/failure_kind/session,
    даёт подсказки по видам отказа, предлагает S1–S4 и выполняет выбранное. Решает пользователь.
  - `.claude-plugin/marketplace.json` в корне: репозиторий — маркетплейс для обоих харнессов.
  - `crates/aae-front/tests/harness_plugin.rs`, 2 теста в `mise run ci`. Первый проверяет форму манифестов:
    маркетплейс → источник → `plugin.json`, имена, kebab-case, версия, frontmatter `name`/`description` у каждого
    скилла. Второй проверяет команды: каждая строка `aae …` из ```sh парсится `Cli::try_parse_from` вместе с флагами,
    каждая `aae …` в тексте — через `has_command`. Мутации (`--continue`, `formulas list`, имя скилла, имя плагина)
    краснят тесты.
  - Документы: раздел установки в `docs/guide/recovery.md`, абзац в `docs/architecture.md`.
  - Формат сверен вживую в изолированных HOME (без модели), детали в `notes/plugin-formats.md`:
    - `claude plugin validate` для корня и для `harness-plugin` — «Validation passed» без предупреждений;
    - `claude plugin marketplace add` + `install aae@aae` и `codex plugin marketplace add` + `plugin add aae@aae`
      ставят плагин, `SKILL.md` попадает в кэш обоих.
- Жел. 3: код стенда WP-99 и логи в `.loop/cycles/112/notes/` удалены через `git rm`. Удаление застейджено и уйдёт
  коммитом цикла; в истории файлы остаются.
- Гейт: `scripts/loop/checks 113` на HEAD `f73ab51` — pass, 35 s, nextest 559/559.

## Не получилось и почему

—

## Начато и не закончено (файлы, точка остановки, следующий шаг)

—

## Замечания предыдущего ревью: что закрыто, что нет

- Обязательных не было.
- Закрыты Жел. 1 (через адаптер, ядро не тронуто), Жел. 2 и Жел. 3.
- Жел. 4 (путь `held` в codex без теста) — без изменений: фейкового app-server нет.
- Жел. 5 (колонка `class` в `run list`) — не брал, вне плана.

## Риски и что не проверено

- Codex читает манифесты Claude как совместимый («legacy») формат; своего `.codex-plugin/` нет. Запись в журнале.
- Загрузку скилла в живой сессии не проверял (нужна модель). Установку из удалённого git (`kvokka/aae`) тоже не
  проверял, только из локального репозитория.

## Контекст на момент отчёта: ~95k

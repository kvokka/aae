# Цикл 124 — coder
STATUS: done

## Взято в работу
- Жел. 1 ревью 123: тест `AAE_BEAD` из ядра в `SessionPlan`.
- WP-120 (плагин `context-budget`, M).
- Жел. 4 ревью 123: длинные строки в docs.

## Сделано (коммиты)
- `9cfb026` WP-118: скриптовый драйвер `aae-testkit` кладёт бид плана в окружение сессии (`AAE_BEAD`, как адаптеры);
  тест `beads::a_run_claims_its_bead…` проверяет его. Мутация `row.bead.clone()` → `None` в `machine.rs` теперь
  краснит тест (проверено).
- `6c7d5e9` WP-120: `plugins/context-budget/` — `plugin.toml` (параметры `window` = 200000, `max_compactions` = 3;
  `native.claude.autoCompactWindow` и `native.codex.model_auto_compact_token_limit` из `${params.window}`; харнесс-хуки
  `PostCompact` и `PreToolUse`), `hooks/budget` (sh + jq), `README.md`. Тесты — e2e модуль
  `crates/aae/tests/e2e/context_budget.rs`:
  - сборка конфига через `aae config explain`: переопределённый на агенте `window` дошёл целым числом до нативного
    ключа claude, значение по умолчанию — до codex; хуки с каталогом данных и `max_compactions` у обоих;
  - скрипт хука на входах `PostCompact` claude и codex (фикстуры по эксперименту 9) с настоящим `br` во временном
    трекере и подставным `aae`: компакция субагента не считается; метка `compactions:<n>` заменяет прежнюю
    (`compactions:7`), чужие метки целы; сверх бюджета — `aae finish fail --summary "context budget: 2 compactions"`,
    ответ `continue:false`, после этого `PreToolUse` отвечает `deny` + `continue:false`; без бида — только счёт.
  - 7 мутаций скрипта (фильтр `agent_id`, снятие старой метки, `-gt`→`-ge`, вызов финишера, вывод финишера в stdout,
    `deny`→`allow`, запись счёта) — все краснят тест.
  - Документы: `docs/architecture.md` (первый плагин в `plugins/`), `docs/roadmap.md` (строка «Compaction plugin»).
- `fc220bc` docs: переносы длинных строк (architecture.md, adapters.md ×2, guide/plugin.md) — Жел. 4.
- `17be636` WP-120: cargo fmt теста (первый прогон гейта упал на fmt).
- Гейт: `scripts/loop/checks 124` на `17be636` — pass, 39 s, 628 тестов.

## Не получилось и почему
- Сценарий приёмки без модели для WP-120 не делал: фейки `aae-testkit` всегда отдают 10 токенов `usage`, компакцию не
  вызвать (по WP-116). Доставку до харнесса покажет демо 4 (WP-126). Запись в журнале.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Жел. 1 (тест `AAE_BEAD` в ядре) — закрыто, `9cfb026`.
- Жел. 4 (длинные строки) — закрыто, `fc220bc` (строки 10/15 architecture.md — диаграмма в code block, `adapters.md:51` —
  старая, не трогал).
- Жел. 2 (события до снимка) — решение владельца, не трогал. Жел. 3 (`guide/plugin.md` ссылки) — WP-127.
- Жел. 5 (`${params}` в `[[service]]`) — `context-budget` сервисов не имеет; не решалось.
- Жел. 6 (старые хвосты) — не брал.

## Решение по WP-120 (подробно — журнал, РЕШЕНИЕ цикла 124)
Остановка сверх бюджета — через финишер из хука харнесса, без хука оркестратора на `run.turn_ended_without_outcome`
(у хука оркестратора нет `AAE_PLUGIN_DATA`, где лежит счёт; ядро не меняется). Риск: на claude `deny` +
`continue:false` из `PreToolUse` не замерены живьём (WP-116 мерил только `continue:false`, при котором инструмент
всё же выполняется); если агент после превышения успеет вызвать `aae finish pass`, последний вызов выиграет. Проверит
демо 4.

## Контекст на момент отчёта: ~115k

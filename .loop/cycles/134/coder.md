# Цикл 134 — coder
STATUS: done

## Взято в работу
- «Желательно» 1–2 ревью 133 (тесты `last_comments` у родителей и устаревшего ответа `lineage`).
- WP-139 (`context-budget` для claude: заметка на мягком пределе, отсечка на жёстком), размер M, критичный.
- WP-143 (ответы MCP в TOON), размер S — сделал subcoder, проверил и закоммитил я.

## Сделано (коммиты)
- `ba9ca49` WP-138: тесты — `last_comments` режет и комментарии родителей; ответ `lineage` к прошлому заходу в шаг
  отбрасывается (`bead_comments::an_answer_to_an_earlier_entry_of_the_step_is_dropped`, ручная отправка
  `Msg::Lineage` с `attempt = 1`, пока прогон готовит попытку 3). Мутации из ревью (нет рекурсии `keep_last`; только
  проверка `state`) теперь краснят по тесту каждая.
- `bb05d56` WP-139:
  - `addons/context-budget/`: `addon.toml` (params `soft`/`hard`/`warn_step`/`cut`, `harnesses = ["claude"]`,
    `autoCompactWindow = 1000000`, хуки `SessionStart`, `SubagentStart`, `PreToolUse`, `PostToolUse`, `PreCompact`),
    `hooks/budget` (sh + jq, замер по транскрипту как `loop_guard.py`, свой транскрипт субагента, заметка и повтор
    по `warn_step`, deny `Agent`/`Task` после `soft`, дамп в бид + `finish <cut>` + deny/`continue:false` после
    `hard`, у субагента — без finish, `PreCompact` — block), README;
  - тесты `crates/loop-manager/tests/e2e/context_budget.rs`: explain (параметры, окно, пять хуков), демо 4,
    заметка/повтор/deny субагента, отсечка с `cut = fail|pass` (дамп в бид, один finish, deny дальше), субагент на
    своём транскрипте без finish; в выводе хуков нет числа `hard` (проверка во всех трёх тестах хука).
    Мутации хука (без `warn_step`, без фильтра `<synthetic>`, транскрипт субагента = главный, finish у субагента,
    без блока `PreCompact`) — каждая краснит тест;
  - решение 149 вместо 144 (`decisions.md`, 144 помечено в шапке как заменённое);
  - доки: `docs/guide/addon-development.md` (рабочий пример — полный манифест), `docs/guide/addons.md`,
    `docs/architecture.md`, `docs/roadmap.md` (удалена строка «Compaction addon»), `docs/acceptance.md` строка 20;
  - демо 4 (субагент subcoder): аддон только у `glossarist`, `run.sh` проверяет комментарий агента в биде,
    `RESULT-example.md` удалён, README, `demos/README.md`, заголовок сценария в `checks.rs`;
  - `devloop/`: параметры аддона новые (у coder `cut = "pass"`), промпты говорят о бюджете 300k, не о сжатии —
    иначе пак не загружался; `guides.rs` — под новый пример.

- `e913b3e` WP-143: текст успешного ответа MCP — TOON (`toon` из `loop-manager-contract` публичный, реэкспорт
  через core — без правки `Cargo.lock`); e2e `mcp.rs` сверяет TOON-текст и что это не JSON (мутация «вернуть JSON»
  краснит `a_run_started_over_mcp_is_followed_listed_and_cancelled_there`); `docs/api/mcp.md`, `docs/api/frontends.md`;
  допущение в `assumptions.md` заменено, запись в журнале. `c17_mcp.rs` не менялся: JSON он не разбирает.

## Гейт
- `scripts/loop/checks 134` отказался: в дереве чужая правка `.claude/settings.json` (глубина субагентов 2 → 4). Обход
  не делал (ревью 133), записал БЛОКЕР в журнал. `checks.md` нет.
- Вместо него `mise run ci` прогнан напрямую на HEAD `e913b3e`: exit 0, 44 s, nextest 690/690. Лог:
  `.loop/cycles/134/notes/ci.log`, итог и HEAD: `.loop/cycles/134/notes/ci-head.txt`.

## Не получилось и почему
- «Желательно» 5 ревью 133 (длинные строки в `contracts.md`/`config.md`) не делал: строк длиннее 120 в этих файлах
  десятки, правило не сформулировано; косметика.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Следующее по плану: WP-140 (codex), WP-141 (`dcg`), затем WP-142.
- Не проверено вживую: доходит ли `additionalContext` от `SubagentStart` до субагента claude (журнал).
- Риск живого прогона демо 4: glossarist может объявить `finish fail` после сворачивания (журнал, НАХОДКА).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Желательно 1, 2 — закрыты (`ba9ca49`). 3 — прогон критерия 16 не делал (нужна просьба владельца). 4 —
  не делал (мелочь, на усмотрение). 5 — см. выше. 6 — замечание без действия.
- Нарушение (обход `assume-unchanged`): в этом цикле не повторялось.

## Контекст на момент отчёта: ~175k

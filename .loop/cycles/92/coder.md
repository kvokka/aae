# Цикл 92 — coder
STATUS: done

## Взято в работу
WP-76 (M): конфиг пользователя — харнесс без секции, `[agents.<имя>]` вместо `[overrides.*]`, режим разрешений в
`native.<harness>`, вид шага `prompt` → `ai`. Обязательных пунктов у ревью 91 не было.

## Сделано (коммиты)
- `7c36d9f` WP-76: основной пакет.
  - Ядро `config/`: множество харнессов = адаптеры сборки ∪ секции; в `Config::harnesses` с умолчаниями попадают
    харнессы, на которых идут шаги `ai` (`compose::check_runs` их возвращает).
  - `[agents.<имя>]` без `base` — правка агента (бывший слой оверрайда, поле `edits`), с `base` — вариант; в риге
    `agents.<имя>` — таблица настроек либо строка-вариант; неизвестное имя без `base` — ошибка с подсказкой;
    `max_runs` в правке — ошибка. Слои в «объясни конфиг»: `orchestrator|project|rig [agents]`.
  - `permission_mode` убран из `Settings`/`EffectiveConfig`. claude: `permission_mode` из `native.claude` →
    `--permission-mode` (вне JSON `--settings`); codex: `approval_policy` из `native.codex` → `approvalPolicy` треда
    (не в `-c` процесса). Тесты адаптеров на это (subcoder).
  - `StepKind::Prompt` → `Ai`, `kind = "ai"` в формулах, `step_kind` `"ai"` в состоянии/событиях/API/OpenAPI,
    миграция 20 — умолчание `'ai'`; проза «prompt step» → «ai step» в коде и доках.
  - Старый ключ — ошибка одной строкой с подсказкой (`config/files.rs`, `RETIRED`).
  - Демо: пустые `[harness.*]` убраны из `config.toml`, `[agents.*]`, `[settings.native.claude|codex]`; README демо 1.
    `aae config check` обоих демо — exit 0.
  - Доки: `docs/design/config.md`, `docs/guide/first-formula.md`, `first-project.md`, `contracts.md` (helper).
  - Критерий 27 — строка в `acceptance-ci` (`checks.rs`, 5 тестов ядра); новые тесты:
    `a_harness_of_the_build_needs_no_section`, `a_retired_key_is_one_line_that_says_where_it_went`, ошибки правок в
    `variant_errors`.
- `bd76e5e`, `6b62665` — READINESS: `ver`, «Config assembly» 10 → 8 (корневая половина критерия 27 — WP-77; демо нового
  вида живьём — WP-84), строка 27 в колонке критериев.
- `447a9f1` — `config/load.rs` под лимит 400 строк (подсказки → `files.rs`, таблица `agents` рига → `variants.rs`).
- `04879ce` — подсказка о `permission_mode` в ядре без имён харнессов (`scripts/core-boundary.sh`, решение 43
  спеки): «its own key in `[settings.native.<harness>]`»; ключи claude/codex названы в `docs/design/config.md`.
- `e6fd0da` — READINESS `ver=04879ce`.
- Проверки: `scripts/loop/checks 92` на `e6fd0da` — pass: ci 68 с, nextest 468/468, acceptance 29 pass / 0 fail /
  9 not run (новая строка 27 — pass). Первый прогон упал на `core-boundary` (исправлено выше); два ранних запуска в
  фоне убиты при завершении хода (exit 144), итог не писали.

## Не получилось и почему
- grep из «Готово, когда» WP-76 находит `permission_mode` и как ключ блока `native.claude` (так назван ключ claude,
  см. журнал 16:30, п. 1). Остальные попадания — подсказки, их тесты, спека.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующий пакет — WP-77 (критичный, начать с плана expert).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Жел. 1 (чужая правка Rust 1.100) — уже закоммичена владельцем (`da79bd0`), вопрос снят.
- Жел. 2 (фон демо 2) — пакет WP-83.
- Жел. 3–5 — не трогал (логи прошлого цикла, ссылка в READINESS на прошлый прогон — текст MEASURE переписан частично).

## Контекст на момент отчёта: ~190k

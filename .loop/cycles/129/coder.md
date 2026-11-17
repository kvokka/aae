# Цикл 129 — coder
STATUS: done

## Взято в работу
WP-128 (plugin → addon, решение 145) целиком; попутно Жел. 1 ревью 128.

## Сделано (коммиты)
- `85ffc72` WP-128: переименование по решению 145.
  - `git mv`: `crates/aae-plugins` → `crates/aae-addons`, `plugins/` → `addons/` (`plugin.toml` → `addon.toml`),
    `operator-plugin/` → `plugin/`, `config/plugin.rs` → `addon.rs`, `config_plugins.rs` → `config_addons.rs`,
    `plugin_attach.rs` → `addon_attach.rs`, `front/tests/operator_plugin.rs` → `plugin.rs`,
    руководства `addons.md`, `addon-development.md`.
  - Код (82 файла, по списку, без нативных): `[[addon]]`, `addons = [...]`, `requires_addons`, `AAE_ADDON_*`,
    `addon.hook_failed` и поле `addon`, `/v1/addons`, `aae addon list`, `Layer::Addon`, `<state>/addons`,
    часть бюджета `addons`, `core-boundary.sh`. Артикли a → an поправлены.
  - Нативное оставлено руками: `aae-claude/src/part.rs` (весь плагин сессии), `tests/process.rs:95`,
    `aae-front/tests/plugin.rs` (только путь `plugin/`), маркетплейсы (только путь `./plugin`).
  - OpenAPI перегенерирован из кода (`AAE_UPDATE_OPENAPI=1`), «plugin» в нём нет.
  - docs/, README, демо 4 (конфиги, run.sh, mise.toml, README; в RESULT-example — только ключи, слой, путь) —
    субагент subcoder, проверено мной (диф decisions/assumptions прочитан целиком: меняется только слово).
  - Жел. 1: `rules-code.md` и критерий 22 `acceptance.md` — маркеры проверяются по репозиторию, вывод агентов демо
    (`demos/*/*-example/`) не проверяется.
- Число тестов: `cargo nextest list --locked --workspace | wc -l` — 668 до и 668 после; гейт 658/658 (как в цикле 128).
- Гейт: `scripts/loop/checks 129` на `85ffc72` — pass, 34 s.

## Оставшиеся вхождения «plugin» (все нативные или записи)
- `crates/aae-claude/src/part.rs` (10) — плагин сессии claude: `--plugin-dir`, `.claude-plugin/plugin.json`.
- `crates/aae-claude/src/tests/process.rs:95` — тест `no skills, no plugin` (`--plugin-dir`).
- `crates/aae-front/tests/plugin.rs` (19) — тест нативного плагина оператора, маркетплейсы, `.claude-plugin`.
- `plugin/plugin.json`, `.claude-plugin/marketplace.json`, `.agents/plugins/marketplace.json` — манифесты нативного
  плагина; `.claude/settings.json` — `enabledPlugins` (харнесс, не трогается).
- `docs/agents/decisions.md` (4) — решения 125, 141 (импорт нативных плагинов), 142 (плагин харнесса оператора),
  строка 145 как есть.
- `docs/architecture.md` (3), `docs/api/frontends.md:118`, `docs/guide/recovery.md` (4) — плагин оператора `plugin/`,
  маркетплейсы, `claude plugin install`.
- `docs/design/contracts.md` (3) — `--plugin-dir`, skills plugin процесса claude, синхронизация плагинов app-server
  codex; `docs/design/config.md:951`, `docs/guide/addon-development.md:105` — каталог плагина Claude Code.
- `docs/design/experiments.md` (8) — эксперименты с `--plugin-dir`, встроенные плагины claude, плагины opencode,
  маркер `src=.loop/research/2026-11-18-plugins` (имя каталога-записи).
- `demos/4-context-budget/RESULT-example.md:59` — проза записи прогона «the plugin counted» (run.sh теперь печатает
  «the addon counted»; запись не переписывается).
- `demos/4-context-budget/library-example/` (GLOSSARY 16, INDEX 13) — вывод агентов живого прогона, не трогается
  (в т. ч. `operator-plugin`, `aae-plugins`, `AAE_PLUGIN_*` — единственные вхождения второго grep, кроме строки 145).
- Полный список docs — `.loop/cycles/129/notes/wp128-docs.md`.

## Не получилось и почему
- Жел. 2 ревью 128 (`checks.rs:319` срез вместо split): строки не освободились, `checks.rs` по-прежнему 399 — не делал.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Жел. 1 — закрыто. Жел. 2 — нет (см. выше). Жел. 3 — не требует действий. Жел. 4 (хвосты ревью 127) — не брал,
  вне пакета.

## Контекст на момент отчёта: ~60k

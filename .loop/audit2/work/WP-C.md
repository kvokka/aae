# WP-C — конфиг агента, строгость конфига, цепочки по бидам
STATUS: partial (сделаны шаги 1–4 и D 7; B 8, D 8 и D 9 вне зоны — описаны ниже)

## Сделано

| Находка | Коммит | Тест (краснеет без правки) |
|---|---|---|
| D 1 — промпт роли не доставлялся | 6ee02dc | `crates/aae/tests/role_prompt.rs` (демо `snake.frontend`: `prompt.md` → `EffectiveConfig.system_prompt` слоя agent → `split()` claude и codex; проверено мутацией), `tests/config_layers.rs` (explain), `tests/config.rs` (оба сразу — ошибка) |
| B 2 — эпик обрывался на 11-м биде | 8daf328 | `crates/aae-testkit/tests/epic.rs`: 15 бидов, каждый заблокирован предыдущим, триггер на `bead.ready`; без правки — `trigger.skipped{depth 11}` (проверено мутацией). Петля A→B→A по `run.*` — существующий `triggers.rs::an_event_chain_…_stops_at_the_depth_limit…`, проходит |
| D 3.2 — MCP ключи не того вида | 67d6f96 | `tests/config_strict.rs::mcp_keys_of_the_other_kind…` |
| D 3.3 — неизвестное `event` хука | 67d6f96 | `config_strict.rs::a_harness_hook_on_an_unknown_event…` |
| D 3.4 — `default-policy` вне диапазона | 5a05415 | `aae-plugins/src/tests.rs` (`crash_loop=0`, `auth_backoff_base_s=0`, `backoff_cap_s=-5`, `rapid_s=-1`) |
| D 3.5 — `vars` затеняют `rig.name/path` | 67d6f96 | `config_strict.rs::a_rig_var_may_not_hide…` |
| D 3.6 — формулы в подкаталогах | 67d6f96 | `config_strict.rs::a_formula_in_a_subdirectory_is_loaded` (выбрано «грузить»: обход в `items.rs::formulas`) |
| D 11 (часть) — `plugins` в `[agent]` плагина | 67d6f96 | `config_strict.rs::a_plugin_that_attaches_plugins_is_told_so` |
| D 4 — `${AAE_PLUGIN_DIR}` | 9442b38 | `config_services.rs::a_plugin_names_its_own_files…` (env, mcp command/env, hook command, строки `native`) |
| D 7 — пин git-источника | 0a6f685 | `config_sources.rs::a_checkout_moved_off_its_pin_is_an_error` (загрузчик читает `.git/HEAD` checkout, git не запускает) |

Решения [Д] — в журнале: промпт роли = `system_prompt` слоя агента (оба сразу — ошибка); `bead.ready` не наращивает
глубину. Список событий хуков — 33 события claude 2.1 + 3 события codex (`Interrupt`, `PreModelRequest`,
`PostModelRequest`), из `aae-context/04`.

Прогоны: `cargo nextest -p aae-core -p aae-plugins -p aae-testkit` — 239/239; `-p aae` — 49/49; clippy
`aae-core`, `aae-plugins`, `aae-testkit` чистый. Живые модели не запускались.

## Не сделано и почему

- **B 8 (риг из метки бида)** — правка в `run/msg.rs`, `run/queue.rs`/`beads.rs`, `aae-front` API и CLI: вне зоны.
  Записано в журнал как ВОПРОС; предложение владельцу — отдельный пакет или сузить [Д] §9.3.
- **D 8 (таймаут встроенного `worktree`)** — таймаут и запуск в блокирующей задаче живут в `run/hooks.rs`
  (`HOOK_TIMEOUT_MS`, `bundled`), не в зоне. Нужно: потолок 300 с для встроенного хука создания и запуск `git`
  через `process::run` с группой и маркером.
- **D 9 (отказ наблюдателя трекера)** — `crates/aae/src/daemon.rs:150-153`, не в зоне. Нужно: событие
  `tracker.sync_failed{op:"watch"}` и строка в `doctor`/`status`.
- **D 7, остаток**: правки файлов внутри checkout (дописанный `formulas/evil.toml`) не ловятся — для этого нужен
  `git status`, место ему — `aae fetch` («kept») или `doctor` (`crates/aae/src`, не в зоне). В коде — `shortcut:`.
- **D 3.1 (имя харнесса)** — по поручению не моё.
- **D 3.7/3.8** (`permission_mode`/`effort` родные, `version` любая строка) — не брались (не в списке шагов).
- Файлы, не являющиеся каталогами, в `agents/` по-прежнему молча пропускаются (например, `agents/README.md`) — это
  оставлено сознательно.

## Нужные правки чужих файлов и спеки

- `docs/design/contracts.md` (6.1 и строка про depth в 5.x): добавить «`bead.ready` не добавляет уровень глубины».
- Спека §9.5 (оригинал «защита от петель по глубине „прогон породил прогон“»): уточнить, что готовый бид не считается
  порождённым прогоном.
- `docs/guide/plugin.md`: строка «каталог плагина Claude Code (`--plugin-dir`) плагином aae не передаётся — skills,
  хуки и MCP через `[agent]`» (в `config.md` уже есть).
- Мой новый тест `crates/aae/tests/role_prompt.rs` лежит в `crates/aae/tests/` (новый файл; `src/` не тронут):
  только там доступны оба адаптера.

## Инцидент

Коммит 6ee02dc захватил незакоммиченные правки исполнителя документации в `docs/design/config.md` (`git add`
всего файла). Их правки сохранены в истории, ничего не потеряно; дальше общие документы стейджились по кускам.
Запись в журнале.

## Изменённые файлы

`crates/aae-core/src/config/{items,mod,files,load,sources}.rs`, `crates/aae-core/src/run/triggers.rs`,
`crates/aae-core/src/tests/{config,config_layers,config_services,config_sources,config_strict,mod}.rs`,
`crates/aae-plugins/src/{lib,policy,tests}.rs`, `crates/aae-testkit/tests/epic.rs`, `crates/aae/tests/role_prompt.rs`,
`docs/design/config.md`, `.loop/JOURNAL.md` (не закоммичен — общий файл).

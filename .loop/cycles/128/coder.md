# Цикл 128 — coder
STATUS: done

## Взято в работу
- Обяз. 1 ревью 127 (битая ссылка в decisions.md).
- Жел. 1 и 2 ревью 127 (тесты на выжившие мутации) — через subcoder.
- WP-125 (демо 4 `context-budget`).
- WP-126 (живой прогон демо 4) — в остатке бюджета.

## Сделано (коммиты)
- `8050935` Decisions: заголовок раздела Plugins ссылается на `guide/plugins.md` и `guide/plugin-development.md`.
  `grep -rn 'guide/plugin\.md' docs README.md crates` пуст.
- `9af4e74` Тесты ревью 127:
  - харнесс `a_resume_writes_the_role_files_of_its_targets` (resume с новым промптом цели → ребёнок получает новые
    инструкции) — краснеет на мутации `roles::write` → `part.0.clone()` в `resume_session` (по таймауту 30 s);
  - цель с пустым `system_prompt` в `a_thread_gets_a_role_per_target_…` — краснеет без фильтра пустой роли;
  - сервис без MCP у цели проходит — краснеет без `mcp.is_some()`;
  - выключенный хук у цели проходит. Мутация фильтра `enabled` в `not_inherited` не убиваема: `by_key`
    (assemble.rs) выбрасывает выключенные хуки ещё при сборке. Фильтр недостижим — удалён (меньше кода).
- `d9eff5c`, `be06af9`, `54c37ca` WP-125: `demos/4-context-budget/`:
  - пак `study`: агенты `glossarist` (claude) и `indexer` (codex), формулы `glossary` и `index` по одному ai-шагу;
  - проект `library` без rig'ов, корень `../library`, куда `run.sh` копирует `docs/` aae (~100k токенов);
  - плагин объявлен и подключён глобально в `root/config.toml` (`window = 100000` — минимум claude,
    `max_compactions = 8`); у `indexer` в `aae.toml` переопределён `window = 50000` с комментарием;
  - `run.sh`: две параллельные заявки, два бида; демо падает, если у бида нет метки `compactions:`; RESULT.md —
    параметры из `explain`, метки, исход, счётчики сессий;
  - тест `demo_4_attaches_the_plugin_to_both_agents_and_gives_the_codex_one_its_own_window` (context_budget.rs),
    краснеет при снятии переопределения;
  - строка приёмки «demo 4» (Launch::Request), `demos_load` теперь грузит каждый демо из `demos/` (а не список);
    `checks.rs` ужат до 399 строк (было 409 после добавления);
  - docs/acceptance.md (критерий 20), demos/README.md, README плагина — ссылка на демо.
- `59b81eb` WP-126: живой прогон демо 4 (`EXAMPLE=1`, `AAE=target/debug/aae`, с первой попытки): passed за 356 s;
  claude `haiku` — 5 компакций, codex `luna` — 3; метки `compactions:5` и `compactions:3` на своих бидах.
  `RESULT-example.md`, `library-example/` (GLOSSARY.md, INDEX.md). Комментарий в `aae.toml` («codex компактит
  чаще») прогон опроверг — исправлен. Версия в маркере — `54c37ca-dirty` (грязным был только `.loop/`).
- `d61a72a` `scripts/budget.sh` не ищет MEASURE-маркеры в `demos/*-example/`: глоссарий агента процитировал шаблон
  маркера, и бюджет упал. Маркеры `RESULT-example.md` по-прежнему проверяются.
- Гейт `scripts/loop/checks 128` на `d61a72a`: pass, 34 s, 658 тестов.

## Не получилось и почему
- Нет.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего. Следующий пакет — WP-128 (plugin → addon, M, вставлен владельцем; брать отдельно). Он переименует и
  демо 4 (`[[plugin]]`, `plugins = …` в `root/config.toml`, `aae.toml`, README, `run.sh`), а `RESULT-example.md`
  останется со старыми словами — это запись прогона.
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (`8050935`).
- Жел. 1 — закрыто. Жел. 2 — закрыто (3 мутации убиты, 4-я — недостижимый код, удалён).
- Жел. 3 (имя цели `max_threads`/`max_depth`), Жел. 4 (хвосты 125/126) — не брал.
## Контекст на момент отчёта: ~125k

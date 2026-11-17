# Цикл 127 — coder
STATUS: done

## Взято в работу
- Жел. 1 и 3 ревью 126.
- WP-123 (субагенты codex, M).
- WP-127 (руководства по плагинам, S) — после WP-123, запас контекста позволил.

## Сделано (коммиты)
- bad3ecc — Жел. 1: тест `the_servers_of_the_agents_a_session_may_call_are_bound_with_it` даёт `qa` плагин с
  `[[service]]` без MCP; мутация (удалить `retain` в `services.rs`) краснит тест (3 ≠ 2). Жел. 3: rustdoc
  `materialize` перенесён.
- 6ae4715 — WP-123, загрузочная проверка: у `Adapter` флаг `subagents_inherit` (codex — true, claude — false); цель
  codex с MCP-сервером, сервисом с MCP или хуком, которых нет у вызывающего, — ошибка загрузки. Тест на три случая
  (и что claude их принимает); три мутации (весь флаг, сервисы, хуки) краснят тест. config.md, adapters.md.
- 719f3d0 — WP-123, доставка: цели — роли треда `config.agents.<тип> = {description, config_file}` поверх
  `native.agents`; файлы ролей `<state>/codex/roles/<hash>.toml` (developer_instructions, model,
  model_reasoning_effort, `[[skills.config]]` выключает скиллы процесса, которых у цели нет) пишет
  `roles::write` перед `thread/start`/`thread/resume` под замком `instances`. Скиллы целей — в process part (отпечаток),
  роли — на тред (не в отпечатке). Фейк: `Reply::Call(name, args)`. Тесты: часть адаптера (роли, скиллы, native
  `agents`, повтор на resume, без целей — ни `agents`, ни `roles`); харнесс на настоящем codex 0.169 + фейк, v1 и v2:
  ребёнок получает инструкции роли, модель роли и только скиллы цели, сессия видит тип, описание и все скиллы.
  Мутации: без `[[skills.config]]`, без `developer_instructions`, без `config.agents` — краснят. Документы: config.md
  «Subagents», contracts 4.6, adapters.md, experiments.md (эксперимент 10 с MEASURE).
  План и протокол эксперимента expert — `.loop/cycles/127/notes/wp123-plan.md`; его отступления от строки пакета
  (allow-list «только свои» невозможен — роль только выключает; версию multi-agent aae не задаёт) — в журнале.

- 4b4c929 — WP-127: `docs/guide/plugin.md` заменён на `docs/guide/plugins.md` (подключить: декларация, слои,
  параметры, `harnesses`, explain; субагенты, граф, что оставлено харнессу) и `docs/guide/plugin-development.md`
  (формат, обе стороны, параметры, `${AAE_PLUGIN_DATA}`, тесты, перенос в свой репозиторий, разбор `context-budget`).
  Ссылки обновлены (README, docs/README, first-formula, rustdoc в 7 файлах), глоссарий overview (plugin, subagent),
  roadmap (бюджет и компакция — `plugins/context-budget`). Тест `crates/aae/tests/e2e/guides.rs`: TOML-блоки
  руководств парсятся, конфиг из них проходит `config check`, `explain` показывает параметры плагина, блок
  `plugin.toml` `context-budget` совпадает с файлом, хуки-примеры дают ожидаемый вывод. Писал subcoder, я проверил
  руководство по использованию и поправил абзац про MCP/хуки субагента codex.
- c8e2c59 — первый прогон гейта упал на `scripts/budget.sh` (два тестовых файла > 400 строк): тест codex-цели
  перенесён в `config_targets.rs`, тест сервисов ужат. Повторный `scripts/loop/checks 127` на c8e2c59 — pass, 38 s,
  656 тестов.

## Не получилось и почему
- `docs/agents/decisions.md:129` ссылается на удалённый `docs/guide/plugin.md`; файл решений не правлю — запись в
  журнал (НАХОДКА).
- В руководствах команды из текста (`aae plugin list`, `aae agent list`) тестом не проверяются — сверены grep'ом.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Остались WP-125 (демо 4, M) и WP-126 (живой прогон, S).
## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных нет. Жел. 1, 3 — закрыты. Жел. 2 (дедупликация id) — оставлено, как разрешило ревью. Жел. 4
  (вопрос по решению 143) и Жел. 5 (хвосты) — без изменений.
## Риски и непроверенное (WP-123)
- Имя цели, совпадающее с ключом таблицы `agents` codex (`max_threads`, `max_depth`), перекроет лимит — не
  проверяется.
- Дочерние треды codex адаптер не закрывает (в v1 `spawn_agent` возвращается сразу) — находка в журнале (expert).
- Заменяет ли `config` треда так же `mcp_servers`/`hooks` из `native.codex` — не проверено.
## Контекст на момент отчёта: ~150k

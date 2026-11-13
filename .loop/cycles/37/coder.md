# Цикл 37 — coder
STATUS: done

## Взято в работу
- Жел. 1 ревью 36 (тест ответа `requestUserInput`), жел. 2 (строка о dedupe `check_instances`).
- WP-32b (плагины: `PATH`, фрагменты промпта, `worktree` на проекте, `continue_from`).
- WP-36 (паки/плагины из git с lock, файл рабочего пространства bv); git-часть — субагентом, проверена и закоммичена мной.

## Сделано (коммиты)
- `b4f1cda` WP-29b: ответ codex на серверный запрос вынесен в `reply()`, unit-тест: два вопроса, каждый id
  получает текст, `id` ответа = id запроса.
- `bc312e9` WP-32b:
  - `plugin.toml` `prompt = "<файл>"` — текст после system prompt подключивших агентов; `bin/` плагина — в начало
    `PATH` (`config/assemble.rs::contribute`); пример `aae-plugin-review` получил `prompt.md`; сценарий 15 на
    настоящем claude видит фрагмент и `command -v require-word` только у подключившего агента (`--ci 15` — pass).
  - `[builtin.<name>]` в `aae.toml` проекта заменяет таблицу оркестратора для прогонов проекта
    (`check.rs::project_bundled`, `hooks.rs`, `/v1/plugins?project=`, проверка параметров в `daemon.rs::load`).
  - `continue_from`: прогон начинается в workdir источника (`queue.rs`); `worktree` возвращает worktree источника на
    его ветке и удаляет по окончании (тест на git в `aae-plugins`; `aae-testkit/tests/worktree.rs` — туда перенесён
    тест worktree из `hooks.rs`, `until` — в `common`, в `services.rs` свой хелпер переименован в `until_count`).
  - Документы: `config.md`, `plugin.md`, `contracts.md` (строка `continue_from`), READINESS, openapi.
- `0e02fda` WP-36: `.bv/workspace.yaml` рядом с конфигом оркестратора при старте демона (имя, абсолютный путь,
  префикс br + `-`); тест `crates/aae/tests/tracker.rs`; вручную bv 0.25.0 `--workspace` читает файл (loaded 1).
- `353fb7d` WP-36: `{ git, rev }` для паков, `git`+`rev` для `[[plugin]]`, `aae.lock`, `aae fetch [--update]`
  (`crates/aae/src/fetch.rs`), разрешение источников при загрузке без git (`config/sources.rs`), тесты
  `tests/config_sources.rs` и `crates/aae/tests/fetch.rs` (fetched → kept → updated, git-плагин и git-пак грузятся,
  закрепление удалённого источника уходит, плохая ревизия — ненулевой код); docs; строка о dedupe `check_instances`
  в `docs/design/event-model.md`.
- Проверки: `mise run ci` на итоговом дереве — EXIT 0, nextest 276/276; бюджет: всего 12938/30000, ядро
  7902/12000; `load.rs` 386 строк.

## Не получилось и почему
- Включение `worktree` ключом в `agent.toml`/`formula.toml` не делалось: остаются списки `agents`/`formulas`
  в `[builtin.worktree]`, теперь и на уровне проекта (решение в журнале).
- Тест `lagged` живого просмотра (жел. 3) не сделан: нужен драйвер с живым потоком в тестовом мире; дешёвого пути нет.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Известные ограничения git-источников: таблица пака с лишним ключом даёт общую ошибку serde про untagged enum;
checkout должен иметь `pack.toml`/`plugin.toml` в корне (подкаталогов нет); `--update` клонирует заново даже без
смены коммита; сеть не проверялась (только локальные репозитории).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Жел. 1 (тест `requestUserInput`) — закрыт `b4f1cda`.
- Жел. 2 (dedupe `check_instances`) — описано в `docs/design/event-model.md` (`353fb7d`).
- Жел. 3 (`lagged`), 4 (хвосты ревью 35), 5 (сведение READINESS с баллами ревью) — открыты. READINESS поднят только по
  затронутым строкам: br 7, поставка 7, worktree 6.

## Контекст на момент отчёта: ~185k

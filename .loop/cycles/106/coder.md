# Цикл 106 — coder
STATUS: done

## Взято в работу
WP-91 (M, первым по плану), затем WP-87, WP-88, WP-89 (S). WP-90 не брал: по плану он идёт последним и один.

## Сделано (коммиты)
- `1a72678` WP-91: worktree одного репозитория создаются и удаляются по одному.
- `8cd5473` WP-91: тесты worktree плагинов вынесены в `crates/aae-plugins/src/tests/worktree.rs` (новый тест довёл
  `tests.rs` до 417 строк при пределе 400).
- `7358746` WP-87: один интеграционный бинарь `harness` у `aae-claude` и у `aae-codex`.
- `ccdb981` WP-88 (1): `test = false` у `[lib]` `aae-br` и у `[[bin]]` `mcp-probe`.
- `8466054` WP-88 (2): семь встроенных `mod tests { … }` вынесены в `tests.rs`; `scripts/core-boundary.sh` пропускает
  любой `tests.rs` ядра.
- `048d2e3` WP-89: `default-members` без `aae-acceptance`; `--workspace` в `ci` и в clippy раннера.

### WP-91 — причина и доказательство
- Падение до правки (первый же запуск): `run.failed` с `"reason":"hook_failed:worktree"`, `workdir` — каталог рига.
- Причина: глобальный git config этой машины включает `worktree.useRelativePaths`. Первый `git worktree add` в
  репозитории пишет в его config `extensions.relativeWorktrees`. Второй `add`, идущий одновременно, падает:
  `error: could not lock config file config: File exists` / `fatal: unable to set extensions.relativeWorktrees setting`.
  Отдельно от aae, на свежих репозиториях, две одновременные пары `prune` + `add`: с настройкой падают 23 из 40, без
  неё 0 из 40.
- Почему раньше не падало: до `ca7799d` второй прогон ждал первого в очереди.
- Починка в продукте: `hold()` в `crates/aae-plugins/src/worktree.rs`. Это блокировка ОС (`File::try_lock`) на сам
  каталог git common dir; файла блокировки нет. Её держат `create` и `remove`, ждут в пределах таймаута хука.
  Очередь прогонов не возвращалась.
- Тест на причину: `runs_of_one_repository_prepared_at_once_each_get_a_worktree`. В нём 8 потоков готовят прогоны
  одного свежего репозитория с локальным `worktree.useRelativePaths = true`. Без правки падает в 9 из 10 запусков, с
  правкой — 0 из 30.
- Счёт мигавшего теста после правки:
  - 200 запусков в одиночку — 0 падений;
  - 50 запусков под полной загрузкой CPU — 0 падений. Нагрузка — 9 процессов `sha256sum /dev/zero` на 9 ядрах, а не
    параллельная сборка: вторая сборка в той же `target/` ждала бы блокировки cargo, а отдельный `CARGO_TARGET_DIR`
    запрещён.
- Все 9 тестов `worktree` зелёные. Документ: `docs/design/config.md`, раздел о встроенном `worktree`.
- `mise run ci` (`scripts/loop/checks 106`) — три раза подряд pass на `048d2e3`.

### WP-87
- Тесты не потерялись: 74 до и 74 после (36 у claude, 38 у codex), имена отличаются только префиксом модуля.
  Списки: `notes/wp87-list-before.txt`, `notes/wp87-list-after.txt`.
- Тестовых бинарей стало 17 (было 20).
- Фильтр критерия 11: `-p aae-claude -p aae-codex -p aae-br -E binary(contract)|test(/^contract::/)|test(/^models::/)`
  выбирает 41 тест (`notes/wp87-c11-after.txt`).
- Пути исправлены в `docs/design/adapters.md` и `docs/harness-updates.md`.
- Замер записи на диск после `touch crates/aae-core/src/lib.rs`, в установившемся режиме:
  - до — 610 и 577 МБ;
  - после — 559 и 559 МБ.
  Первый замер сразу после `git checkout` завышен (618–775), его не считаю.

### WP-88
- Тестов осталось 548: 547 и новый тест WP-91.
- Бинарей стало 15 (было 17).
- `git grep -nA1 '#\[cfg(test)\]' -- 'crates/*/src' | grep -E 'mod [a-z_]+ \{'` ничего не находит.
- После `touch crates/aae-front/src/api/tests.rs` пересобирается только `Compiling aae-front`, без `aae` и
  `aae-acceptance` (`notes/wp88-front-tests-touch.txt`).

### WP-89
Вывод — `notes/wp89.txt`.
- После `touch crates/aae-acceptance/src/main.rs` обычный `cargo nextest run --no-run` ничего не компилирует, и в
  списке бинарей нет `aae-acceptance`.
- С `--workspace` компилируется `aae-acceptance`. Сразу после этого обычный `--no-run` не печатает ни одного
  `Compiling`.
- В `ci` 538 тестов (без chaos).
- Описано в `docs/guide/development.md` и в правиле `docs/agents/rules-tests.md`.
- Проверка на чистом `target` не делалась: она стёрла бы общую сборку. Вместо неё — проверка через `touch`, описанная
  выше.

## Не получилось и почему
Нет.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующий по плану — WP-90 (критичный, один в цикле, начинается с плана expert), затем WP-92…

## Замечания предыдущего ревью: что закрыто, что нет
- «Обязательно» в ревью 105 не было.
- «Желательно» 1–4 не брались: они вне плана раунда.
- В журнале две записи: НАХОДКА о причине WP-91 и ВОПРОС о командах харнесса после `default-members`.

## Контекст на момент отчёта: ~135k

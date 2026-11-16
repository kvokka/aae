# Цикл 107 — coder
STATUS: done

## Взято в работу
- Доделка WP-91 по «Желательно» 1–2 ревью 106 (S, ~10k).
- WP-90 (M, критичный) — с плана expert (`notes/wp90-plan.md`). Других пакетов не брал.

## Сделано (коммиты)
- `ffa27dc` WP-91: тест гонки — 6 свежих репозиториев по 3 прогона. Мутация (убран `hold` в `create`): красный
  20 из 20 (было 11 из 20); без мутации 20 из 20 зелёные. `hold` в `remove` оставлен, довод — в rustdoc `hold`:
  `git worktree prune` в `create` удаляет регистрацию worktree без каталога, а это состояние посреди
  `git worktree remove`. Документ (`config.md`, `worktree`) уже говорит про обе операции — не менялся.
- `55cb2a2` WP-90: новый крейт `crates/aae-contract` — `config/`, `driver*`, `tracker`, `orphans*`, `cron`,
  `template`, новый `event.rs` (`SCHEMA`, `envelope`, `rfc3339_ms`, `parse_rfc3339_ms` из `journal.rs`),
  `BoxFuture` в корне, `MAX_ATTEMPTS` в `config` (в `run` — реэкспорт). `aae-core` — движок, реэкспортирует
  `BoxFuture, config, driver, orphans, template, tracker`; `aae`, `aae-front`, `aae-acceptance`, тесты
  `aae-testkit/tests` импортов не меняли. `pub(crate)` → `pub`: `config::{input_variable, upper}`,
  `orphans::{sweep, sweep_all, listens_alone}`, `envelope`. Тесты переехали с модулями (93 в `aae-contract`, 35 в
  `aae-core`); `a_project_trigger_without_a_rig_runs_in_the_root` зовёт `Reader` — остался в `aae-core` со своей
  фикстурой из пяти файлов. `checks.rs`: в критерии 27 добавлен `-p aae-contract`, в шести остальных он заменил
  `-p aae-core`. Скрипты `core-boundary.sh`, `select-biased.sh`, `budget.sh`, `default-members` — на оба крейта.
  Документы: `architecture.md` (границы, таблица крейтов), `development.md`, `rules-code.md`.
- `2ccc4fb` WP-90: `aae-claude`, `aae-codex`, `aae-br`, `aae-plugins`, библиотека `aae-testkit` зависят от
  `aae-contract` (у тесткита `aae-core` — только dev). `clippy.toml`, `adapters.md`, `contracts.md:865` — пути
  `aae_contract::`.

Проверки «готово» WP-90:
- После `touch crates/aae-core/src/run/mod.rs` и `cargo nextest run --locked --no-run`:
  - до (`ffa27dc`): Compiling aae-core, aae-testkit, aae-plugins, aae-claude, aae-br, aae-codex, aae-front, aae;
    запись 527 МБ;
  - после (`2ccc4fb`): Compiling aae-core, aae-front, aae-testkit, aae; запись 324 МБ (−38 %).
- `cargo nextest list --locked --workspace`: 548 тестов, `diff` со списком до (`notes/tests-before.txt`) после замены
  пакета `aae-core|aae-contract` — пусто.
- Фильтры `checks.rs` с `-p aae-co…`: 12/12, 12/12, 7/7, 8/8, 15/15, 7/7, 14/14 (ожидаемое/выбранное).
- `scripts/core-boundary.sh` с `// codex` в `aae-contract/src/cron.rs` и в `aae-core/src/clock.rs` — exit 1 с именем
  файла; правки откачены.
- `cargo tree` (normal+dev) у четырёх адаптеров/плагинов и normal у `aae-testkit` — `aae-core` нет.
- Гейт: `scripts/loop/checks 107` — HEAD `2ccc4fb`, pass, 30 s, nextest 538/538.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
—

## Замечания предыдущего ревью: что закрыто, что нет
- «Обязательно» — не было.
- Жел. 1 (тест WP-91 ловит регрессию в половине запусков) — закрыто, 20/20 под мутацией.
- Жел. 2 (`hold` в `remove`) — закрыто доводом в rustdoc, блокировка оставлена.
- Жел. 3 (тесты читают глобальный git config; без git ≥ 2.48 тест молча ничего не проверяет) — не делал.
- Жел. 4 — ждёт владельца. Новые вопросы WP-90 (имя `aae-contract`, текст решения 43) — запись expert в журнале;
  предложение CI-сторожа зависимостей — моя запись в журнале.

## Контекст на момент отчёта: ~120k

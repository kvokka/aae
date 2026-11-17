# Цикл 120 — coder
STATUS: done

## Взято в работу
- Обязательное ревью 119 (п. 1 и 2) — WP-109; Жел. 1 и 6 — документом.
- WP-110 (демо 3 на `gh` хоста, живой прогон), WP-114, WP-115.

## Сделано (коммиты)
- `db95e03` WP-109: check-скрипт, чей `env` ссылается на отсутствующую у демона переменную, не запускается; прогон
  сразу (без повтора и без политики) кончается `failed{infrastructure}` с reason `config_error: check script: env `K`:
  …` (ключ и переменная). Новое сообщение `Msg::CheckRefused` → `check_refused` в `repeat.rs`; общий страж
  `checking`. Тест `a_check_script_with_a_missing_variable_fails_the_run_as_config_error` (max_attempts 2 — повтора нет).
  Документы: config.md «Environment values», contracts (`finishing`), assumptions.md (пункт отступления убран).
- `5708818`, `5822a48` WP-109: тесты охран литералов — `a_tracker_directory_with_a_dollar_stays_itself_in_env`
  (places.rs:47) и `a_dollar_in_a_plugin_bin_and_the_daemons_path_stays_itself` (assemble.rs:224 и 232; тест
  перезапускает себя с `PATH`, где есть `$`, через `orphans::output_std` — `set_var` под `forbid(unsafe)` нельзя).
  Мутации всех трёх охран краснят тесты, откат проверен.
- `4948c8c` WP-110: из `run.sh` убраны свой логин `gh`, обёртка `.aae/bin/gh`; `GITHUB_TOKEN` берётся из окружения
  или `gh auth token` и экспортируется демону; `[exec] env` в `root/config.toml`: `GITHUB_TOKEN = "${GITHUB_TOKEN}"`
  и литерал `MY_AWESOME_ENV_VAR = "something"`; `owner` — `env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }`; шаг `merge`
  печатает литерал; `trap` до запуска демона и на `INT TERM`; shellcheck чистый (строка 17 — `find | grep -q`).
  `review_gate.rs`: конфиг теста берёт `[exec]` демо, демон получает `GITHUB_TOKEN` через обёртку `dir/bin/aae`,
  тест проверяет вывод `merge` (мутация литерала краснит). README обновлён.

- `fec0613` WP-109 (ревью 119, Жел. 1 и 6): config.md — check-скрипт ai-шага не получает `env` хуков
  `run.preparing` (он идёт в сессию агента); `inherit_env` берёт только переменные с UTF-8 именем и значением;
  абзац check-скрипта называет `config_error`.
- `1a02903` WP-114: решение 43 называет `aae-contract` и `aae-core` и запрет зависимости адаптеров от `aae-core`
  (по разрешению владельца в плане); `scripts/core-boundary.sh` проверяет `cargo tree -e normal,build` у
  `aae-claude`, `aae-codex`, `aae-br`, `aae-plugins`. Мутация (aae-core в зависимостях aae-br) — гейт красный с
  понятным текстом; откат. Dev-зависимость через `aae-testkit` разрешена (тесты адаптеров).
  Документы: architecture.md, guide/development.md.

- `66af9cf` WP-110: живой прогон демо 3 (`luna`/`terra`) из чистого worktree HEAD `4948c8c`: `passed` за 420 s,
  2 раунда; MEASURE `aae-0.1.0@4948c8c` без `-dirty`; PR #3 во всех трёх репозиториях смерджены; шаг `merge` вывел
  `MY_AWESOME_ENV_VAR is something`; значения токена нет ни в одном файле каталога демо (grep по значению).
  `RESULT-example.md`, `*-example/` заменены, абзац README о прогоне автора обновлён. `server.py` в примере агент
  написал с shebang — выставлен `+x` (иначе pre-commit не пускает).
- `366903c`, `+fmt` WP-115: сценарий «demo 3» (Launch::Request, 1 прогон) в `checks.rs`; тест
  `every_demo_has_a_live_run` (каждый каталог demos/ с `run.sh` имеет живой прогон; мутация заголовка краснит);
  docs/acceptance.md (таблица команд, критерий 20), guide/development.md (gh для демо 3). Без логина `gh` и без
  `GITHUB_TOKEN` `run.sh` сразу падает с «log gh in or set GITHUB_TOKEN», раннер показывает хвост вывода.
  Сам раннер приёмки не запускался.
- Гейт: `scripts/loop/checks 120` — pass, 49 s, nextest 617/617 (первый прогон упал на `cargo fmt --all`:
  `aae-acceptance` не в default-members, починено отдельным коммитом).

## Не получилось и почему
- Нет.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Открыты WP-111, WP-112, WP-113 (не брались: бюджет).
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (`db95e03`). Обяз. 2 — закрыто (`5708818`, `5822a48`).
- Жел. 1 (env хуков `run.preparing` и check-скрипт ai-шага) — описано в config.md (`fec0613`), код не менялся.
- Жел. 6 (UTF-8 при `inherit_env`) — оговорка в config.md. Жел. 2 (имя переменной в `run show`) — для check-скрипта
  теперь в reason прогона; для exec-шага не трогал. Жел. 3–5 (время гейта, флейки) — к WP-112, не брал.

## Контекст на момент отчёта: ~185k

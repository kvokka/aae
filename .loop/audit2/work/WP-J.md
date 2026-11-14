# WP-J — свести H и I, демо «планировщик — исполнители», полный прогон проверок
STATUS: done, кроме живого демо 2 (codex не залогинен на хосте)

## Шаг 1 — свести

1. `f1f5054` — патч `queue.patch` WP-H применён как есть: прогон варианта под своим именем, ключ ворот модели при
   создании прогона (прогон на закрытой модели ждёт в `queued`), `max_runs` через `Config::max_runs`. Тесты
   `aae-core` + `aae-testkit` 268/268 (два новых в `model_gate.rs`).
2. `19ba45b` — фронтенд: `GET /v1/agents` отдаёт **итоговые** `harness` и `provider` агента в проекте (через
   `Config::effective` без рига и формулы; `null`, если слои их не решают); `Agent.harness`/`provider` —
   `Option<String>`. `aae config explain` и `GET /v1/config/explain`: `--formula <f> --rig <r>` вместо `--agent`
   → `Config::for_run`, видны слои пака, формулы, варианта; ответ получил поле `agent` (агент или вариант прогона).
   Без формулы — как раньше. OpenAPI перегенерирован. Тесты: `crates/aae/tests/cli.rs` (explain по формуле: слой
   `formula shop.go`, агент; без `--rig` — ошибка; список агентов — `claude`/`anthropic` выведенный).
   `docs/design/config.md` — абзац про `--formula`.
3. `f4e33b1` — сокращения codex: `terra` → `gpt-6.1-terra`, `sol` → `gpt-6.1-sol` (настоящие id codex 0.168.0).
   Новый контрактный тест `crates/aae-codex/tests/models.rs`: настоящий `codex app-server`, пустой `CODEX_HOME`,
   `initialize` → `model/list {includeHidden}` (без модели и без аккаунта): каждая цель таблицы есть в списке и
   `upgrade = null`. Краснел на старой таблице (`terra` → `gpt-6-1-terra` не найден). Запись РЕШЕНИЕ в журнале.
   Критерий 11 приёмки считает этот тест (`binary(models)`, 36 → 37; `ec0bfe3`, `bf45900`).
4. `aebe729` — ссылки на §2.1/§2.2 в `crates/`, `docs/` (кроме `docs/init`), `scripts/core-boundary.sh` заменены на
   «spec §2, decision N» по списку WP-S (сделал helper, номера сверены с таблицей §2; 21 файл). Остались длинные
   (>120) строки markdown в `docs/design/{config,contracts,invariants}.md` — их там много и до правки, CI их не
   проверяет. `de81b62` — `docs/guide/development.md`: критерий 1 проверяется локальным полным прогоном
   (`mise run acceptance-release`), тег ради него не заводится. `.github/workflows/release.yml` существует только
   ради критерия 1 (сборка четырёх целей на теге `v*`) — оставлен как есть.
5. Перед шагом 2: clippy всего workspace чисто, `cargo nextest run --workspace` 414/414.

## Шаг 2 — демо (раздел 6 проекта)

- `55cb7ff` — `demos/snake`, старые `snake-claude`, `snake-mixed`, `demos/config.toml` удалены. Новое:
  - `demos/dev-pack/` — пак `dev`: агенты `planner` (делит запрос на независимые задачи по ригам и раздаёт) и `coder`
    (пишет код и проверяет перед «работает»); в `agent.toml` умолчания пака: `provider = "anthropic"`,
    `model = "sonnet"`, `effort = "medium"`; харнесс не назван. Формулы `plan` (вход `request`) и `code` (вход
    `task`) — только процесс: риги — `aae --json project list --project {{ project.name }}` (в шаблоне списка ригов
    нет, `PROMPT_ROOTS` даёт только `project.name|path`), самодостаточные задачи с общим интерфейсом в обеих,
    последовательный запуск, если части нужен результат другой, `aae run start … dev.code --input task=…`, цикл
    `aae run wait … --timeout 100s` (124 — ещё раз), одна переформулировка провала, затем `fail`; `code` — сделать,
    проверить как сказано, не оставить процессов, `finish pass --data` с данными для соседей. Слов задачи в паке нет.
  - `demos/<demo>/root/`: `config.toml` (`projects = ["."]`, харнессы), `aae.toml` (`name`, `packs =
    ["../../dev-pack"]`, риги `root` = `.`, `backend`, `frontend`; `[settings] inherit_host = false`,
    `permission_mode = "bypassPermissions"`; `[overrides.planner|coder] model = "haiku"`; триггер `smoke-after-plan`
    на `run.passed` формулы `dev.plan` → exec-формула `smoke` локального пака на риге `root`), `REQUEST.md` (рассказ
    пользователя: порт 8765/`PORT`, `GET`/`POST /scores`, `scores.txt`, `index.html`, `server.py`, как он проверит),
    `smoke.sh` (проверка как прежде, пути `../backend`, `../frontend`), `formulas/smoke.toml`.
  - `snake-mixed/root/aae.toml` отличается только `name`, вариантом `[agents.codex-coder]` (`base = "dev.coder"`,
    `provider = "openai"`, `model = "luna"`, `permission_mode = "never"`) и `agents = { coder = "codex-coder" }` на
    риге `backend`; `config.toml` добавляет `[harness.codex]`. README показывает второй путь (оверрайд рига) и
    `aae config explain --formula dev.code --rig backend`.
  - `demos/run.sh`: копия пака и `root/` во временный каталог, пустые риги, свой демон, изолированные
    `XDG_STATE_HOME/XDG_CONFIG_HOME/XDG_CACHE_HOME`, `AAE_SOCKET`; старт `aae run start <demo> root dev.plan --input
    request="$(cat REQUEST.md)"`; ожидание — `aae run wait <plan> --timeout 40m`, затем `aae run wait <smoke>`
    (без опроса); `RESULT.md`: маркер `MEASURE` (дата, `aae-<версия>@<sha>`, версии claude/codex, время, число
    прогонов планировщика), токены из `run.passed`, таблица прогонов с родителями, исход планировщика.
  - `demos/README.md`, README обоих демо, `demos/mise.toml` (`check` — оба `root/config.toml`), строка в `README.md`.
  - Приёмка: строка `demos load` критерия 20 проверяет оба `root/config.toml`; заголовки живых строк обновлены.
  - `crates/aae/tests/role_prompt.rs`: загрузка обоих демо с адаптерами — `snake-claude`: `dev.plan`/`dev.code` на
    всех ригах → claude, `haiku` из слоя `project override`; `snake-mixed`: на `backend` агент `codex-coder`, харнесс
    `codex` (`derived`), модель `gpt-6-luna`, заданная как `luna`; на `frontend` — `dev.coder` на claude. Роль
    `coder` доходит до обоих адаптеров.
- `b09a833`, `5823da4` — правки вывода `run.sh` (имя рига `root`, счёт прогонов без smoke, токены) и снимок демо 1.
- `60523e0` — гайды `first-project.md`, `first-formula.md`: формула — процесс, задача — вход; настройки агента —
  умолчания роли, оверрайд проекта/рига, вариант; `explain --formula`.

## Живые прогоны

| Демо | Попытка | HEAD | Итог | Время | Дочерних | Токены (вход/выход) |
|---|---|---|---|---|---|---|
| snake-claude | 1 | `bf45900` | passed (smoke passed) | 96 с | 2 (backend, frontend параллельно) | 52 / 23 291 |
| snake-claude | 2 | `b09a833` | passed (smoke passed), снимок | 188 с | 2 | 64 / 33 267 |
| snake-mixed | — | — | не запускалось: `codex login status` → Not logged in | — | — | — |

Стоимость claude в событиях не сообщает (только токены). Формулы `plan`/`code` с первой попытки, правок не
потребовалось. Записи в журнале: НАХОДКА ×2, БЛОКЕР (codex login).

## Шаг 3 — проверки

На закоммиченном HEAD `5823da4` (последнее изменение `crates/` — `bf45900`, маркер `READINESS.md` `ver=bf45900`):
- `mise run ci`: clippy, fmt, deny, machete, unused-pub, core-boundary, budget — чисто; nextest **415/415 passed**;
  budget total 16049 / 30000.
- `mise run acceptance-ci`: **21 of 30 rows passed, 0 failed, 9 not run** (не запускаются в `--ci`: 1, живые 2, 4a,
  8, 17 и две строки 20). Критерий 11 — 37/37 (было 36, + `models.rs`), 17 — 7/7 (не менялся), 22 — чисто.
- `docs/READINESS.md`: баллы не менялись (компоненты с живыми критериями — 6; 10 невозможно, пока строка демо 2 не
  запускалась); снято устаревшее «ворота модели узнаются только из сбоя»; абзац оценки переписан.

## Не сделано / владельцу

- Живое демо 2 `snake-mixed`: нужен `codex login`, затем `cd demos && mise run snake-mixed` (снимок и
  `RESULT.md` появятся в `demos/snake-mixed/`). Сейчас там `.gitkeep` в пустых ригах.
- Находка: пути ригов не канонизируются (`.../root/./.`, `.../root/./../backend`) — предложение канонизировать при
  загрузке (ядро). В `run.sh` обойдено для вывода.
- `release.yml` оставлен (только ради критерия 1 на теге `v*`); тег не заводится.
- Правки спеки (исполнителю документации): §15 критерий 20 — строка загрузки теперь «оба `demos/<demo>/root/config.toml`»;
  §12 — `aae config explain --formula <f> --rig <r>`, поле `agent` в ответе; `GET /v1/agents` — итоговые
  `harness`/`provider` (nullable); §7.5 — сокращения codex: `gpt-6.1-terra`, `gpt-6.1-sol` (через точку).

## Изменённые файлы

`crates/aae-core/src/run/queue.rs`, `crates/aae-testkit/tests/model_gate.rs`, `crates/aae-front/src/api/{config,replies}.rs`,
`crates/aae-front/src/cli/config.rs`, `crates/aae/tests/{cli,role_prompt}.rs`, `crates/aae-codex/src/adapter.rs`,
`crates/aae-codex/tests/models.rs` (новый), `crates/aae-acceptance/src/checks.rs`, 20 файлов со ссылками на §2.1/§2.2,
`docs/api/openapi.json`, `docs/design/config.md`, `docs/guide/{development,first-project,first-formula}.md`,
`docs/READINESS.md`, `README.md`, `demos/**` (пересобраны целиком).

## Контекст на момент отчёта: ~150k

# WP-B — харнессы как адаптеры: формализация границы и баги адаптеров
STATUS: done (кроме пунктов «не сделано» ниже)

## По шагам поручения

### 1. Одна таблица адаптеров, ноль `match` по имени — сделано (4c34f93)
- `aae-core/src/driver/adapter.rs` (новый): `Adapter{name, tested, new: fn(&Setup)->Result<Arc<dyn Driver>,String>,
  doctor: fn(DoctorAgents)->Vec<Check>}`, `Setup{binary, options, state, socket}`, `Check`, общие формулировки
  `login_check`, `host_mcp_check`. В `driver.rs` — только `mod`/`pub use`.
- `aae-claude/src/adapter.rs`, `aae-codex/src/adapter.rs`: `pub const ADAPTER`; разбор `[harness.codex]`
  (`max_sessions`, `instance_idle_s`) переехал в codex; проверки `doctor` (логин, MCP хоста, песочница codex) —
  в адаптерах. `host_key`/`unconfined` больше не `pub`. Версии — одна константа `TESTED` на адаптер (и в
  `Capabilities.versions`, и в `Adapter.tested`); хак `capabilities(&ProcessPart(null))` в корне удалён.
- `crates/aae/src/harness.rs`: `const ADAPTERS = [&aae_claude::ADAPTER, &aae_codex::ADAPTER]`, поиск по имени;
  `doctor.rs` обходит таблицу без имён. `grep -rniE 'codex|claude' crates/aae/src --include=*.rs` → одна строка
  таблицы (плюс `daemon_tests.rs` — чужой тест, строка `"codex"` в нём). Ветки «по умолчанию = codex» исчезли.
- Сигнатуры `driver()`/`tested()` корня сохранены (их зовёт `daemon.rs`, чужая зона).

### 2. `[harness.<без адаптера>]` — ошибка конфига — сделано (4c34f93)
`load()` (его зовут `config check`, `doctor`, старт и перезагрузка демона) даёт
`` `harness.opencode`: aae has no adapter for harness `opencode`; known: claude, codex ``; ошибка конструктора
адаптера (неверный ключ) — тоже ошибка загрузки. Агент с харнессом без таблицы уже был ошибкой ядра; агент на
таблице без адаптера ловится через ошибку таблицы. Тест `a_harness_with_no_adapter_is_a_config_error_naming_the_known_ones`
(краснел на старом коде — проверено `git stash`).
Не сделано: `config check` не зовёт `split()` для каждого агента (C-harness 8, вторая половина): ошибки формы
native/хуков/MCP по-прежнему видны только на старте прогона. Причина: сервисы на момент проверки не связаны, MCP
сервисов в `split` ещё без URL — нужен отдельный режим проверки, это за пределами шага.

### 3. Баги адаптеров
- **C-harness 1** (транскрипт claude при `CLAUDE_CONFIG_DIR`) — сделано: `transcript()` берёт `part::host_dir()`,
  тот же источник, что `daemon_vars`. Тест — дополнение `the_transcript_tells_where_the_prompt_stands`, мутацией
  краснеет. **Коммит ушёл в чужой ef24271** (гонка общего индекса, запись в журнале).
- **C-harness 5** (ложный «login ok») — сделано (4c34f93): ключ из окружения `doctor` не считается; ok — файл
  логина хоста, у codex ещё переменная `env_key` провайдера хоста (её адаптер передаёт), либо у каждого агента
  харнесса свой ключ в `env`; иначе warning с перечнем агентов без ключа. Тест
  `a_login_counts_only_when_a_session_the_daemon_starts_gets_it` (краснел на старом коде).
- **C-harness 6** (`adapter_crash`) — сделано (20a74e8): `driver::child::watch` ждёт задачу-читателя; паника →
  `Failure{adapter_crash, scope instance}` без сессии (ядро раздаёт всем живым попыткам экземпляра), адаптер
  забывает экземпляр (codex ещё сбрасывает ждущие вызовы), группа процессов получает KILL, затем `Exited`. Оба
  адаптера вызывают одинаково. Тест — в ядре (`a_panic_of_the_reader_fails_every_session_of_the_instance_and_kills_it`);
  на уровне адаптера панику не вызвать без тестового крючка в продукте — не делал.
- **A-journal 2 / C-harness 13** (`cancel_turn` claude) — сделано (7c5bd90): `interrupted()` ставит флаг только при
  открытом ходе; сам `interrupt` пишется всегда (безвреден, и закрывает гонку «промпт отправлен, ещё не queued»).
  Тест `an_interrupt_after_the_turn_ended_does_not_cancel_the_next_turn`, мутацией краснеет.
- **D 5** (каталоги сессий claude) — сделано частично (28bec9f): `settings.json`, `mcp.json` (токены),
  `plugin/` теперь в `<state>/claude/<session>/<instance>/` и удаляются при выходе процесса (resume пишет заново).
  `config/` изолированной сессии (транскрипт) остаётся — безопасного момента в интерфейсе нет; описано в
  `contracts.md` 4.6 и в журнале с предложением (`forget_session` или команда чистки). При падении демона каталог
  экземпляра остаётся. Тест — дополнение `a_process_that_dies_in_a_turn_…`, мутацией краснеет.
- **C-harness 9** (`codex_home`) — не удалён: ядро сопоставляет события с попыткой по тексту JSON ссылки, смена
  формы рвёт попытки через обновление. Описано в журнале и в `contracts.md` 4.2.
- **C-harness 12** — `scripts/core-boundary.sh` не моя зона. Предложение: `grep -i` и список имён из `ADAPTERS`.
  `HOST_NETWORK_ENV` с `NODE_EXTRA_CA_CERTS` оставлен в ядре (его зовут оба адаптера).
- **D 6** (br через mise в каталоге проекта, явный путь) — не сделано: нужны `daemon.rs` (`binary("br", None)`) и
  ключ конфига — чужие зоны. Нужно: ключ пути br в конфиге и `mise which` с `current_dir(project.root)` по проекту в
  `daemon.rs` и `doctor.rs`; `harness::binary` для этого достаточно дать параметр каталога.

### 4. codex: команда после конца хода — сделано (dc3fcd8)
Сценарий s14 переписан: `exec_command` на `sleep 14 && touch bg` уступает ход (codex по умолчанию через ~10 с),
ход кончается `Idle{background:true}`, поздний `item/completed` даёт `Idle{background:false}`, ход сам не
начинается. Поддельному провайдеру хватило `Reply::Turn`. Плюс unit-тест потока. Мутация «background = false» роняет
оба. Строка 14 таблицы codex в `docs/harness-updates.md` исправлена. Цена: s14 идёт ~16 с.

### 5. Хуки харнесса у codex — сделано (fcf5dd9)
Опыт на настоящем codex с поддельным провайдером: хук `Stop` треда срабатывает, исполняется через shell, но
`AAE_RUN`/токена не видит. Адаптер ставит перед командой хука `export AAE_RUN=… AAE_ATTEMPT_TOKEN=… <актёр>=…; `
(значения в одинарных кавычках). Контрактный тест `a_thread_hook_runs_with_the_attempt_environment` (краснел до правки).
Компромисс: токен виден в argv хука — решение [Д] в журнале.

### 6. Общий код адаптеров — сделано (af44004)
`aae-core/src/driver/child.rs`: `spawn` (pipes, своя группа, pid+start_time, ошибка запуска = config_error),
`base_env` (переменные демона, env агента, `AAE_INSTANCE`, `AAE_SOCKET`), `Events` (общий `seq`), `alive`/`health`,
лестница остановки `stop`, `config_error`, `watch`. Три копии `config_error`, две — лестницы, health, сборщика
событий, каркаса запуска и окружения удалены. Не обобщал: таблицы формулировок, автоматы потока, сворачивание
`Activity` и обрезку `raw` в `Failure` (в `stream.rs` — различаются окружением), стенд контрактных тестов `Live`
(чужой крейт `aae-testkit`).

### 7. `docs/design/adapters.md` — сделано (bb232ed), 90 строк
Что экспортирует адаптер, что ждёт ядро (в т. ч. `PromptAcked`, пара `TurnEnded`+`Idle` без фона,
`retried_by_harness`, сопоставление `SessionRef` по тексту, подстановка портов/токенов сервисов, группа процессов),
контрактный набор, «Known limits of the driver interface». `contracts.md` 4 и 9 приведены к коду.

## Что адаптеры шлют, а ядро не читает (C-harness 7, не удалялось)
- `Normalized::Usage{input_tokens, output_tokens, rate_limit}` и `RateLimit` (claude `stream.rs:254`, codex
  `stream.rs:328-338`) — ядро только двигает сторож тишины (`events.rs` ветка `_`).
- `Normalized::Retrying{attempt, delay, text}` — то же; у codex `delay` всегда 0.
- `Normalized::TurnStarted` — для ядра равно активности.
- `DriverEvent.seq` — не читается нигде вне адаптеров.
- `Health::Degraded` — не конструируется; `PromptState::Unanswered|Answered` — ядро различает только `Absent`.
- `Source::Harness` — вне тестов не используется; `Failure.scope` — только в журнал.
- `Capabilities.versions` — после шага 1 его не читает никто (корень берёт `Adapter.tested`); адаптеры заполняют
  той же константой. Убирать вместе с остальными полями — ломающее изменение для `aae-testkit`.

## Изменённые файлы
`crates/aae-core/src/driver.rs` (только `mod`/`pub use`), `crates/aae-core/src/driver/{adapter,child}.rs` (новые),
`crates/aae-claude/src/{adapter.rs (новый),lib.rs,part.rs,stream.rs,stream/tests.rs,tests.rs}`,
`crates/aae-codex/{Cargo.toml (+serde),src/adapter.rs (новый),src/lib.rs,src/part.rs,src/part/tests.rs,src/rpc.rs,
src/tests.rs,tests/contract.rs,tests/common/mod.rs}`, `crates/aae/src/{harness,doctor}.rs`, `crates/aae/tests/doctor.rs`,
`Cargo.lock`, `docs/design/{adapters.md (новый),contracts.md}`, `docs/harness-updates.md`; журнал — 4 записи.

## Правки чужих файлов и спеки (предложения)
- `README.md`: ссылка на `docs/design/adapters.md` рядом с contracts.
- `scripts/core-boundary.sh`, `scripts/budget.sh`: имена/бюджеты адаптеров — из таблицы, `grep -i`.
- `crates/aae/src/daemon.rs` + конфиг: путь br и `mise which` по проекту (D 6).
- Спека §7.1: «`adapter_crash` порождает адаптер при панике задачи-читателя»; там же устаревшие `session.started` и
  две удалённые способности (C-harness 10).

## Проверки
clippy `-p aae-core -p aae-claude -p aae-codex -p aae --all-targets -D warnings` — чисто; nextest `aae-claude`,
`aae-codex`, `aae` — 118/118, `aae-core` — 91/91. Контрактные сценарии гоняют настоящие claude/codex с поддельными
провайдерами; живые модели не запускались.

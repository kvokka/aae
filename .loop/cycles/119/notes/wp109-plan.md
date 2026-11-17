# WP-109 — план реализации (expert, цикл 119)

Спецификация — решение 137. Ниже: что и где менять, в каком порядке, чем проверять. Номера строк — на HEAD `6ed968a`.

## 0. Что выяснилось при чтении кода (влияет на план)

- `[exec]` в конфиге нет вовсе; `StepKind::Exec { command, timeout }` (`config/formula_file.rs:245`).
- Механизм слоёв оркестратор → проект → формула уже есть: `Given` + `Loader::formula_defaults`
  (`config/defaults.rs`), им сливаются `rounds` и `[inputs]`. `[exec]` идёт тем же путём.
- Снимок exec-шага — JSON `{"command", "env"}` (`run/render.rs:57`), `env` = `{NAME: {"value": ..}}`; сейчас там
  только окружение трекера и то, что добавили хуки `run.preparing` (`run/machine.rs:78`).
- Окружение агента попадает в процесс харнесса в одном месте: `aae_contract::driver::child::base_env`
  (`driver/child.rs:48`), его зовут `aae-claude/src/part.rs:246` и `aae-codex/src/part.rs:213`. Часть процесса
  (`ProcessPart`, из неё отпечаток экземпляра и `account`) собирается раньше, в `split`, из `config.env` как есть.
  Значит: разрешать в `base_env` — и часть процесса, и отпечаток, и хэш аккаунта остаются с `${NAME}` без единой
  правки. Решение 43 соблюдено: разборщик живёт в `aae-contract`.
- `FailureKind::ConfigError` есть (contracts 4.4); отказ `start_instance` уже идёт в попытку как `Msg::AttemptFailed` /
  `InstanceFailed` (`run/exec.rs:150,279`). Новый kind не нужен.
- Check-скрипт запускается тем же `RunHandler::command` (`run/exec_step.rs:39`) и берёт `config["env"]` снимка
  своего шага. Для ai-шага это окружение **агента** (снимок — `EffectiveConfig`). См. п. 8.1 — расхождение.
- В снимке уже живут свои заглушки `${AAE_SERVICE_PORT:<id>}` / `${AAE_SERVICE_TOKEN:<id>}`
  (`run/service_proc.rs:51,57`); они подставляются в полезную нагрузку запуска до `split`, до адаптера не доходят —
  с новым разборщиком не пересекаются. Пользователь такую форму написать не сможет (имя с `:` — ошибка загрузки).
- Файлы у потолка 400 строк: `config/items.rs` 395, `run/machine.rs` 398, `run/hooks.rs` 390, `config/load.rs` 379.
  План их почти не трогает; новая логика — в новом модуле.
- Логов в ядре и адаптерах нет (два `eprintln!` в `writer.rs` про панику эффекта). Поверхность `run show` снимок
  (`runs.config`) не отдаёт.

## 1. Типы и слои

Новый модуль `crates/aae-contract/src/config/env.rs` (всё по теме в одном файле, ~110 строк):

- `pub(super) struct ExecFile { inherit_env: Option<bool>, env: BTreeMap<String, String> }` — TOML-форма `[exec]`,
  `#[serde(deny_unknown_fields)]`, `Default`.
- `pub struct ExecEnv { pub inherit_env: Option<Sourced<bool>>, pub env: BTreeMap<String, Sourced<String>> }` —
  слитый результат, `Clone, Debug, Default, PartialEq`. `None` у `inherit_env` = никто не сказал = `false`.
- `ExecEnv::under(&mut self, file: &ExecFile, from: &Layer)` — «взять у слоя то, чего ещё нет»: `inherit_env`, если
  `None`; ключи `env` через `entry().or_insert`. Применяется от сильного к слабому, один метод на все три слоя.
- `ExecEnv::explain(&self) -> String` — строки `exec.inherit_env = true  # project`,
  `exec.env.GH_TOKEN = "${OTHER}"  # orchestrator` (формат как у `EffectiveConfig::explain`).
- разборщик ссылок и `impl Loader { fn env_values(..) }` — п. 2.

Куда добавить поля:

| Что | Где |
|---|---|
| `exec: ExecFile` (`#[serde(default)]`) | `OrchestratorFile`, `ProjectFile` — `config/files.rs` |
| `inherit_env: Option<bool>`, `env: BTreeMap<String,String>` (`#[serde(default)]`) | `StepFile::Exec` — `config/formula_file.rs` (`flatten` с `deny_unknown_fields` не работает — два поля) |
| `env: ExecEnv` | `StepKind::Exec` — `config/formula_file.rs` |
| `exec: ExecEnv` — `[exec]` оркестратора и проекта, для check-скриптов шагов не-exec | `Project` — `config/mod.rs:106` |
| `exec: &'a ExecFile` | `Given` — `config/defaults.rs` |

Слияние:

- Шаг: `Loader::step` (`items.rs:264`) строит свой слой: `ExecEnv::default().under(&шаг, &Layer::Formula(имя формулы))`
  — как `rounds` (`items.rs:218`). Имя формулы в `step()` сейчас не передаётся: добавить параметр. `items.rs`
  после этого ~397 строк; если rustfmt выведет за 400 — унести `Loader::text` (8 строк) в `config/prompt.rs`.
- Оркестратор и проект: в `Loader::formula_defaults` (`defaults.rs`) для каждого шага `StepKind::Exec { env, .. }`:
  `for given in layers.iter().rev() { env.under(given.exec, &given.layer) }` (проект, затем оркестратор — у шага
  своё уже стоит). Это и есть «`env` по ключу, `inherit_env` — последний слой побеждает».
- `Project.exec`: в `Loader::project` (`load.rs:309`) — `under(&raw.exec, Project)`, `under(top.exec, Orchestrator)`.
- Rig слоя `[exec]` не получает: решение называет оркестратор, проект и шаг.

Снимок прогона (`run/render.rs:52`), неразрешённый:

- `exec` = `StepKind::Exec.env` для exec-шага, иначе `project.exec`.
- `env` (карта): окружение трекера (как сейчас), поверх — `exec.env` как `{value, from}`; `inherit` =
  `exec.inherit_env.is_some_and(|s| s.value)`.
- exec: `{"command", "env", "inherit_env"}`; drain и formula: добавить `"inherit_env"` рядом с `"env"`;
  ai: `let mut config = json!(config); config["exec"] = json!({"env": env, "inherit_env": inherit})`
  (`EffectiveConfig` десериализуется без `deny_unknown_fields` — лишний ключ адаптерам и `start_attempt` не мешает).
- `RunHandler::command`: `let exec = config.get("exec").unwrap_or(&config);` — дальше `exec["env"]`,
  `exec["inherit_env"]`. Хуки `run.preparing` и `env_keys` продолжают работать с верхним `env` — не трогаем
  (`machine.rs` на пределе).

## 2. Разборщик ссылок — один, в `aae-contract`

```rust
/// `text` with every `${NAME}` replaced by `var(NAME)` and every `$$` by `$`.
pub fn env_value(text: &str, var: impl Fn(&str) -> Option<String>) -> Result<String, String>
/// `text` as an `env` value that stands for itself.
pub fn env_literal(text: &str) -> String            // text.replace('$', "$$")
```

Экспорт: `pub use env::{ExecEnv, env_literal, env_value};` в `config/mod.rs` (ядро видит через `aae_core::config`,
адаптеры — через `aae_contract::config`).

Грамматика — строгая, без вариантов:

- `$$` → `$`; `${NAME}` → значение, `NAME` = `[A-Za-z_][A-Za-z0-9_]*`; всё прочее — как написано.
- Любой другой `$` — ошибка: одиночный (`$HOME`, `$` в конце), незакрытый `${`, пустое `${}`, недопустимое имя
  (`${A-B}`, `${A:-x}`, `${1A}`). Вложений и значений по умолчанию нет.

Почему строго, а не «одиночный `$` — литерал»: самая вероятная ошибка — `GH_TOKEN = "$GH_TOKEN"` в стиле shell.
При мягком правиле шаг молча получит строку `$GH_TOKEN` и упадёт на авторизации с непонятным текстом; при строгом
— ошибка загрузки в одну строку с подсказкой. Конфигов старого формата нет (первый релиз не вышел).

Текст ошибок (значение никогда не цитируется — в нём может быть секрет с опечаткой):
- синтаксис: ``a `$` is written `$$`, a variable of the daemon `${NAME}` ``;
- нет переменной: ``the daemon has no variable `NAME` ``.

Когда что:

- **Загрузка** — только синтаксис: `Loader::env_values(&mut self, file, what, &BTreeMap<String,String>)` зовёт
  `env_value(v, |_| Some(String::new()))` и на ошибку делает `self.fail(file, "`{what}.{key}`: {e}")`. Точки вызова:
  `Loader::settings` (`load.rs:192` — единственная воронка всех слоёв `Settings`: оркестратор, проект, rig, плагин,
  agent.toml, варианты, правки) с `what = "env"`; `[exec]` оркестратора и проекта (`"exec.env"`); exec-шаг.
  Наличие переменной при загрузке **не** проверяется: `config check` может идти в CLI с другим окружением.
- **Запуск процесса** — `env_value(v, |n| std::env::var(n).ok())` (п. 3).

Значения, которые в `env` кладёт сам aae, — литералы, и после введения разборщика их надо экранировать
`env_literal`, иначе путь с `$` сломает шаг. Все точки (их четыре, других нет — проверено grep'ом по `env`):

| Точка | Правка |
|---|---|
| `Settings::in_plugin` (`files.rs:38`) — `${AAE_PLUGIN_DIR}` в `env` | для `self.env` подставлять `env_literal(dir)`; hook/MCP/native — сырой `dir`, как сейчас |
| `Project::tracker_env` (`places.rs:45`) — оба потребителя (`compose.rs:198`, `render.rs:52`) | отдавать значение уже `env_literal` (итератор `(&String, String)`) |
| `EffectiveConfig::contribute` (`assemble.rs:221-234`) — `PATH` | `env_literal` для `bin/` плагинов и для `PATH` демона, когда слой его не задал |
| решение хука `run.preparing` (`run/decide.rs:195`) | `value: env_literal(value.as_str()?)` — хук даёт текст, не ссылку |

`${AAE_PLUGIN_DIR}`: остаётся подстановкой при загрузке, порядок — проверка синтаксиса (`settings`, форма
`${AAE_PLUGIN_DIR}` — законная ссылка), затем `in_plugin`. Конфликта нет: после подстановки в значении либо
экранированный путь, либо настоящие ссылки пользователя. Вне `[agent]` плагина `${AAE_PLUGIN_DIR}` в `env` — теперь
обычная ссылка на переменную демона, которой нет → `config_error` при запуске с её именем (раньше уходил текстом).
Особого случая не делаем. `AAE_SERVICE_*` (URL с портом, токен) `$` не содержат — экранирование не нужно.

## 3. Разрешение при запуске

### Exec-шаг и check-скрипт (ядро, `run/exec_step.rs`)

`command()` — полезная нагрузка эффекта делится на две группы (сейчас всё в одном `env`):

- `"env"` — значения снимка (шаблоны), `"inherit_env"` — из снимка;
- `"context"` — актор трекера, `AAE_RUN`, `AAE_BEAD`, `AAE_INPUT_*`, `AAE_STEP_*`, `AAE_REMARKS` — литералы
  (в `SUMMARY`/`DATA` `$` обычен, разбирать их нельзя). Актор переезжает сюда; `env.retain(..)` уходят.

`run()` (исполнитель эффекта, строка 178) собирает окружение в порядке решения:

1. `payload["inherit_env"] == true` → всё окружение демона: `std::env::vars_os()` с отбросом не-UTF-8 пар
   (`std::env::vars()` на них паникует; комментарий `shortcut:`), иначе `process::base_env()`;
2. `payload["env"]` через `env_value`; ошибка → см. ниже;
3. `payload["context"]` как есть; 4. `AAE_INSTANCE`, `AAE_SOCKET` (как сейчас).
   Дубли снимает `Command::envs` (последний побеждает) — ручной `retain` не нужен.

Отказ (нет переменной):

- шаг (`check == false`): `Msg::AttemptFailed { run, attempt, failure }`, `Failure::new(ConfigError, Source::Core)`,
  `failure.text = Some("env `KEY`: the daemon has no variable `NAME`")` — путь `launch_failed` → `attempt_failed`
  уже есть (`run/events.rs:86`), `Exit` не расширяется, `exec_done`/`hooks.rs` не трогаются. Дальше решает политика:
  `config_error` → `fail` (у exec-прогона нет ключа агента — без гейта) → `failed{infrastructure}`.
- check-скрипт (`check == true`): `done(payload, Exit::SpawnFailed(text), ..)` — действующее правило config.md
  «скрипт, который не может стартовать, роняет прогон как `infrastructure`», причина `check script: env ..`.
  Отказ попытки здесь нельзя: прогон в `finishing`, политика отправила бы ai-сессию на `resume`.

`process.rs`, `service_proc.rs:75`, `hooks.rs:313` не меняются: сервисы и хуки оркестратора решением 137 не названы.

### Окружение агентов (адаптеры)

- `child::base_env` (`aae-contract/src/driver/child.rs:48`) → `Result<Vec<(String,String)>, Failure>`; каждое
  значение `env` через `env_value(.., |n| std::env::var(n).ok())`, ошибка →
  `config_error(format!("env `{k}`: {e}"))` (сообщение в `raw`, как у прочих `config_error` адаптера).
- `aae-claude/src/part.rs:245` и `aae-codex/src/part.rs:212`: `env()` → `Result<_, Failure>`, внутри `?`.
- `aae-claude/src/lib.rs:99`, `aae-codex/src/lib.rs:89`: посчитать `let env = part::env(..)?;` **до** сборки
  `Command` и до `materialize`, затем `.envs(env)`. `start` уже возвращает `Result<InstanceHandle, Failure>`.
- `split` (`part.rs:58` / `:52`), `account` (`:303` / `:353`), проверки `CREDENTIALS` — без правок: работают по
  наличию ключа и по неразрешённой строке. Хэш аккаунта теперь считается от `${NAME}` — «откуда берётся ключ»,
  что и требуется (contracts 7).
- MCP `env`, `[[service]] env` — вне решения 137, остаются литералами (п. 8.4).

## 4. Где секрет мог бы утечь и чем закрыто

| Место | Состояние |
|---|---|
| снимок `runs.config` | пишет `render.rs` из конфига — только шаблоны; ядро в транзакции окружение демона не читает |
| полезная нагрузка эффектов `exec:*` и `launch` (outbox), реестр экземпляров, отпечаток, `account` | шаблоны: разрешение только в памяти исполнителя (`exec_step::run`, `base_env`) в момент спавна |
| события | `run.preparing` несёт `env_keys` без значений; `run.attempt_failed.failure.text/raw` — имя ключа и имя переменной, значение не цитируется (и в ошибках загрузки тоже) |
| `config explain` (CLI, `/v1/config/explain`) | печатает хранимое: `EffectiveConfig::explain`, `ExecEnv::explain` |
| `run show`, `/v1/runs/*` | снимок не отдают |
| логи демона | ядро и адаптеры значения окружения не печатают; ошибка спавна — текст ОС |
| `inherit_env` | окружение демона читается только в `exec_step::run`, нигде не сериализуется |
| вывод самой команды (`outcome.data.stdout/stderr`, 4 KiB) | печатает секрет команда — он попадёт в журнал; это ответственность формулы, одна фраза в config.md |

Проверка — e2e-тест с маркером (п. 5, E1).

## 5. Тесты

Переменная «есть у демона» в тестах ядра и адаптеров — `CARGO_PKG_NAME` (cargo/nextest ставят её тестовому
процессу; приём уже есть в `aae-codex/src/part/tests.rs:94`). «Нет у демона» — `AAE_TEST_NO_SUCH_VARIABLE`.
`std::env::set_var` нельзя (`unsafe_code = "forbid"`). В e2e демон — дочерний процесс, маркер даётся `.env(..)`.

**A. `crates/aae-contract/src/tests/config_env.rs`** (новый модуль, строка в `tests/mod.rs`)

| Тест | Что проверяет |
|---|---|
| `a_reference_takes_the_variable_and_two_dollars_are_one` | `env_value`: литерал, `${X}`, `a${X}b${Y_2}`, `$$`, `$${X}` → текст `${X}` |
| `a_dollar_that_is_no_reference_is_an_error` | таблица: `$`, `a$b`, `$HOME`, `${`, `${}`, `${1X}`, `${A-B}`, `${A:-b}` — все `Err` |
| `a_missing_variable_is_named_and_the_value_is_not_quoted` | `Err` содержит `NOPE`, не содержит остального текста значения |
| `a_literal_comes_back_as_it_was` | `env_value(&env_literal(s), ..) == s` для `$`, `${X}`, `$$`, `a$b` |
| `exec_layers_merge_env_by_key_and_take_the_strongest_inherit_env` | `[exec]` оркестратора, проекта и ключи шага → `ExecEnv` шага: ключи и `from` каждого слоя, перекрытие по ключу, `inherit_env` от сильнейшего сказавшего; шаг без ключей; `project.exec` |
| `a_malformed_env_value_is_a_load_error_of_its_file_and_key` | `$HOME` в `[settings] env`, в `agent.toml`, в `[exec] env`, в `env` шага — ошибка с файлом и ключом; неизвестный ключ в `[exec]` — ошибка |
| `the_directory_of_a_plugin_with_a_dollar_stays_itself_in_env` | плагин в каталоге `pl$ug`, `env = { D = "${AAE_PLUGIN_DIR}/x" }` → `env_value(effective.env["D"])` = настоящий путь; hook-команда — сырой путь |
| `explain_shows_the_reference` | `EffectiveConfig::explain` → `env.GH_TOKEN = "${OTHER}"  # ..`; `ExecEnv::explain` → строки `exec.*` со слоями |

Правка существующего: `tests/config.rs:170` — в шаблон `StepKind::Exec { .., }` добавить `..`.

**B. `crates/aae-contract/src/driver/child/tests.rs`**: `base_env_resolves_references_and_names_what_is_missing`
— литерал, `${CARGO_PKG_NAME}` под другим именем, `$$`; отсутствующая → `Failure` kind `ConfigError`, в `raw` и ключ,
и имя переменной.

**C. Адаптеры** — `aae-claude/src/tests/process.rs`, `aae-codex/src/part/tests.rs`, по одному тесту
`the_process_part_keeps_the_reference_and_the_process_gets_the_value`: после `split` `part["env"]["GH_TOKEN"] ==
"${CARGO_PKG_NAME}"` (значит, и отпечаток, и `account` от ссылки), `part::env(..)` даёт `aae-claude`/`aae-codex`;
отсутствующая переменная → `Err` `config_error`. Существующие вызовы `part::env` в тестах — `.unwrap()`.

**D. `crates/aae-testkit/tests/core/exec_env.rs`** (новый модуль в `tests/core/main.rs`; реальные команды, реальное
время — как `exec.rs`)

| Тест | Что проверяет |
|---|---|
| `a_step_gets_literals_references_and_a_dollar` | `[exec] env = { LIT = "plain", GH_TOKEN = "${CARGO_PKG_NAME}", PRICE = "$$5" }` → stdout `plain aae-testkit $5` (литерал, ссылка, переименование, `$$`) |
| `the_environment_is_clean_unless_inherit_env` | без ключа `CARGO_PKG_NAME` в шаге пуст; `inherit_env = true` на шаге — виден; `AAE_RUN` и `[exec] env` поверх унаследованного |
| `the_step_is_over_the_project_over_the_orchestrator` | три слоя `env` по ключу; `inherit_env`: оркестратор `true`, проект `false` → не унаследовано |
| `a_variable_the_daemon_lacks_fails_the_step_as_config_error` | `run.attempt_failed`: `failure.kind == "config_error"`, в `text` имя переменной; прогон `failed`; команда не запускалась |
| `the_variables_of_aae_win_over_env` | `[exec] env = { AAE_RUN = "mine" }` → в шаге id прогона |
| `the_snapshot_and_the_effect_keep_the_reference` | в `runs.config` и в полезной нагрузке эффекта `exec:<run>` значение ключа `GH_TOKEN` — ровно `${CARGO_PKG_NAME}` |
| `a_dollar_from_a_hook_reaches_the_command_as_it_is` | хук `run.preparing` даёт `env: {"X": "a$b"}` → шаг печатает `a$b` |

**`crates/aae-testkit/tests/core/step_repeat_more.rs`** (99 строк, есть место):
`the_check_script_of_an_ai_step_gets_the_exec_environment` — `[exec] env = { LIT = "x" }` проекта и `env` агента
`{ AGENT_ONLY = "y" }` → скрипт видит `LIT`, не видит `AGENT_ONLY`; и
`a_check_script_with_a_missing_variable_fails_the_run_as_infrastructure`.

**E. `crates/aae/tests/e2e/exec_env.rs`** (новый модуль в `e2e/main.rs`; демон — `common::command(dir)
.env("AAE_E2E_SECRET", MARKER).stderr(лог)`, как в `stop.rs:38`)

`a_secret_reaches_the_step_and_is_stored_nowhere`: оркестратор `[exec] env = { TOKEN = "${AAE_E2E_SECRET}" }`, агент
`dev` с `env = { GH_TOKEN = "${AAE_E2E_SECRET}" }`; exec-шаг `sh -c 'printf %s "$TOKEN" > token.out'` (в stdout
секрет не печатать). После `passed`: `web/token.out` == маркер, файл удалить; затем маркера нет ни в одном файле
временного каталога (state.db с WAL, журнал, лог демона, кэш), ни в выводе `aae run show <run>` (текст и
`--json`), `aae config explain --project shop --formula ..` и `--agent dev`; в explain есть `${AAE_E2E_SECRET}`.

**Мутации охран → какой тест краснеет**

| Мутация | Тесты |
|---|---|
| исполнитель отдаёт `env` без `env_value` | D1 |
| разрешение перенесено в `command()`/`render` (значение в состоянии) | D6, E |
| `inherit_env` игнорируется / наследуется всегда | D2 |
| `under` заменён перезаписью или слои в другом порядке | A5, D3 |
| нет переменной → пустая строка | A3, B, C, D4 |
| одиночный `$` принят / `$$` не свёрнут | A1, A2 |
| `context` раньше `env` | D5 |
| убран `env_literal` в `decide.rs` / `in_plugin` | D7 / A7 |
| убрана проверка при загрузке | A6 |
| адаптер разрешает в `split` | C |
| check-скрипт ai-шага читает `env` агента | step_repeat_more |
| отказ exec классифицирован как `workspace_failure` | D4 |

## 6. Документы (английский; истории «было» не писать)

`docs/design/config.md`:
- «Settings (any layer)»: строка `env` таблицы → ссылка на новый подраздел `### Environment values` там же:
  литерал или `${NAME}` из окружения демона, `$$` — `$`, любой другой `$` — ошибка загрузки; разрешается при
  запуске процесса, нигде не хранится разрешённым (снимок, журнал, отпечаток, explain показывают `${NAME}`);
  нет переменной → `config_error` с именем; где действует (`env` агентов, `[exec] env`, `env` exec-шага) и где нет
  (MCP `env`, `[[service]] env`); вывод команды попадает в журнал — секрет не печатать.
- «Effective config and explain config»: `env` снимка хранит ссылки; строки `exec.inherit_env` / `exec.env.<NAME>`
  под заголовком exec-шага и под строкой check-скрипта шага другого вида.
- «Orchestrator» и «Project»: в образцы TOML `[exec]` с `inherit_env` и `env` и комментарием про слои.
- «Formula»: в образец exec-шага `inherit_env`, `env`; абзац про exec-шаг — порядок окружения (база `PATH`/`HOME`/
  `LANG` → всё окружение демона при `inherit_env` → `env` слоёв оркестратор → проект → шаг по ключу → переменные
  aae поверх); абзац про check-скрипт: окружение exec-шага — своего шага, если он exec, иначе `[exec]` оркестратора
  и проекта; окружение агента скрипту не передаётся; скрипт с неразрешимой ссылкой не стартует (`infrastructure`).
- «Plugin» (стр. ~817): `${AAE_PLUGIN_DIR}` подставляется при загрузке, в `env` — как литерал.

`docs/design/contracts.md`: 6.2 — строка `preparing (exec)` (порядок окружения, `inherit_env`) и строка
`running (exec) | exit 75 / timeout / spawn error` (+ ссылка на переменную, которой нет → `config_error`);
4.4 — у `config_error` источник `adapter, core (an exec step's env)`; 3.2 — `env` решения `run.preparing` — текст
как есть, `$` в нём не ссылка. `docs/design/adapters.md:92` — `base_env` разрешает ссылки и возвращает `config_error`.
`docs/guide/plugin.md:70` — та же оговорка про `env`. `docs/agents/assumptions.md` — допущения п. 8.

## 7. Порядок коммитов и объём

1. **Ссылки в `env`: разбор, проверка при загрузке, разрешение при запуске.** `config/env.rs` (`env_value`,
   `env_literal`, `Loader::env_values`), вызов в `Loader::settings`, четыре точки `env_literal`, `child::base_env` +
   оба адаптера, `exec_step.rs` (группы `env`/`context`, разрешение, отказ). Тесты A1–A4, A6 (settings), A7, A8
   (агент), B, C, D7. Документы: «Environment values», Effective config, Plugin, adapters.md, guide/plugin.md,
   contracts 3.2/4.4. ~170 строк кода, ~200 тестов.
2. **`[exec]` и ключи exec-шага.** `ExecFile`/`ExecEnv`, поля в файлах, `Given.exec`, `formula_defaults`,
   `Project.exec`, `render.rs`, `inherit_env` в исполнителе, `command()` читает `exec`, explain во фронте
   (`aae-front/src/api/config.rs:208`, +6 строк). Тесты A5, A6 (exec), A8 (exec), D1–D6, step_repeat_more. Документы:
   Orchestrator, Project, Formula, contracts 6.2. ~120 строк кода, ~250 тестов.
3. **e2e-тест с маркером** (E) и запись допущений в `assumptions.md` и журнал.

Каждый коммит зелёный сам по себе (точечные тесты + clippy крейта); `scripts/loop/checks 119` — один раз в конце.
Бюджет: ядро 11 228 → ~11 500 из 18 000. Оценка контекста кодера — 120–150k.

Subcoder параллельно, файлы не пересекаются: (а) правки документов п. 6 — сразу, по этому плану; (б) после
коммита 2 — e2e-тест E (`crates/aae/tests/e2e/exec_env.rs` + строка в `main.rs`), пока кодер пишет D. Два cargo
одновременно не гонять (один `target`, диск почти полон): subcoder (б) собирает после кодера.

## 8. Расхождения и допущения — в журнал (`РЕШЕНИЕ`/`ВОПРОС`) и в `assumptions.md`

1. **Check-скрипт ai-шага сейчас получает окружение агента** (снимок `EffectiveConfig.env`, с ключами API), а
   config.md и принятое допущение говорят «окружение exec-шага», решение 137 — «чистое по умолчанию». Чиним код под
   документ: check-скрипт шага не-exec получает трекер + `[exec]` оркестратора и проекта, окружение агента и
   сервисов — нет. Это изменение поведения; `env` от хука `run.preparing` на ai-шаге идёт сессии, не скрипту.
2. **Ссылка без переменной в check-скрипте** — не `config_error` попытки, а `failed{infrastructure}` с именем
   переменной в причине (правило «скрипт не стартовал»). Для самого exec-шага и для сессии агента — `config_error`.
3. **Синтаксис строгий**: одиночный `$` — ошибка загрузки (обоснование — п. 2).
4. **MCP `env`, `[[service]] env`, хуки оркестратора** решением не названы и ссылок не разрешают; пользователь
   может ждать `${NAME}` и там — вопрос владельцу, до ответа литерал.
5. `${AAE_PLUGIN_DIR}` в `env` вне `[agent]` плагина теперь даёт `config_error` при запуске (раньше — текст).
6. `config_error` сессии закрывает гейт агента до оператора (действующая политика, contracts 10); после того как
   переменную дали демону и перезапустили его, гейт открывает оператор. Не меняем.
7. Конфиги `demos/` загружаются как прежде (`$` в их `env` нет); `demos/` не трогаем — это WP-110.

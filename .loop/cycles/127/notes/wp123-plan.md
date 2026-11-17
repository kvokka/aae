# WP-123 · Субагенты codex — эксперимент и план (expert, цикл 127)

Дата 2026-11-18, codex-cli 0.169.0 (`mise exec -- codex`), linux-arm64 orbstack. Живых моделей — 0: всё на фейковом
Responses API. Код продукта не менялся. Загрузочная ошибка (хуки/MCP цели, которых нет у вызывающего) уже сделана
кодером в `6ae4715` (`subagents_inherit`, `not_inherited`) — в плане ниже её нет.

## 1. Стенд (временный каталог `mktemp -d`, в репо не остаётся)

- `fake_openai.py` — Responses API в форме `aae-testkit/src/openai.rs`; каждое тело запроса пишет в `reqs/<n>.json`.
  Ответ: если последний элемент входа не `function_call_output` и в последнем пользовательском сообщении (или в
  `agent_message`, включая его `encrypted_content`) есть `CALL <tool> <json>` — `function_call` этого инструмента (с
  `namespace`, если инструмент в пространстве имён); иначе текст `OK-<n>`.
- `rig.py` — клиент `codex app-server -c analytics.enabled=false -c thread_unload_delay_secs=0 [доп. -c]` по stdio,
  свежие `HOME`/`CODEX_HOME`, `config.toml` с провайдером `fake` и моделью `gpt-fake`; шаги сценария: `thread/start`
  (с `config`), `turn/start` + ожидание `turn/completed`, `thread/unsubscribe` до `notLoaded`, `thread/resume`.
- Скиллы: три каталога `src/dir-{a,b,c}` с frontmatter `name: sk-{a,b,c}` и маркером в описании; ссылки
  `CODEX_HOME/skills/sk-a`, `…/linkb` (имя ссылки ≠ имя во frontmatter `sk-b`), `…/sk-c` — как кладёт aae.
- Роли: файлы `roles/<имя>.toml` вне `CODEX_HOME`, маркеры `DI-<РОЛЬ>-zz` в `developer_instructions`; тред получает
  `config.agents.<роль> = {description, config_file}`; родителю — `developerInstructions: "DI-PARENT-zz"`.

## 2. Прогоны и что увидел

| Прогон | Настройка | Итог |
|---|---|---|
| s1 | тред `skills.config=[{name:"sk-a",enabled:false}]` | sk-a у треда нет, у соседних тредов процесса есть — выключение на тред работает |
| s1 | то же через `path`: ссылка `CODEX_HOME/skills/sk-a/SKILL.md` / цель ссылки `src/dir-a/SKILL.md` | оба выключают |
| s1 | `name:"linkb"` (имя ссылки) / `name:"sk-b"` (frontmatter) / `path` = каталог без `SKILL.md` | не действует / выключает / не действует |
| s5a | в файле роли `path` через другой симлинк и `..` (`src2/../src2/dir-a/SKILL.md`) | выключает — codex канонизирует путь |
| s2 | родитель выключил sk-c на треде; роль `plain` без `skills.config` (v1) | ребёнок видит a, b — наследует `skills.config` треда родителя; `developer_instructions` роли заменили родительские |
| s2 | роль `enc`: `enabled=true` для sk-c (выключен тредом родителя), `enabled=false` для sk-a | ребёнок видит только b: `false` действует, `true` ничего не включает |
| s3a / s3b | sk-c выключен процессом (`config.toml` / флаг `-c skills.config=[…]`), роль `enabled=true` для sk-c | ребёнок sk-c не видит — `enabled=true` роли не включает и выключенное процессом |
| s4v1, s5a | v1, `agents.max_depth=3`: P → роль A (выключает b) → роль B2 (`enabled=true` для b, выключает a) | внук видит только c: выключения ролей по цепочке складываются, `true` роли B2 не снимает выключение роли A |
| s2 | `config_file` в `CODEX_HOME/agents/inhome.toml` (`model`, `model_reasoning_effort`, DI) | роль применилась (`model=gpt-role`, `effort=low`), но на каждый тред предупреждение «agent role file at … must define a non-empty `name`» — каталог `CODEX_HOME/agents/` сканируется как отдельные роли |
| s2 | `config_file` вне `CODEX_HOME` (абсолютный путь) | работает |
| s2 | роль `kit.qa` (точка в имени, вложенный JSON-ключ) | работает |
| s2 | пустой файл роли / роль без `config_file` | роль есть, ребёнок получает инструкции, модель и скиллы родителя |
| s2, s5a | инлайн `config.agents.inl = {description, developer_instructions}` без файла | ребёнок создан, инструкции родителя — инлайн-ключ молча игнорируется |
| s2 | `config_file` указывает на несуществующий файл | `thread/start` проходит; `configWarning` «Ignoring malformed agent role definition: agents.<r>.config_file must point to an existing file», spawn → `unknown agent_type`; предупреждение повторяется на каждом дочернем треде (файл читается на старте треда и на каждом ребёнке) |
| s5a | `developer_instructions = ""` в файле | роль отброшена: «…developer_instructions cannot be blank» |
| s2 | семь spawn подряд (дети не закрыты) | шесть детей открыты, седьмой: «collab spawn failed: agent thread limit reached» |
| s3a | v1, ребёнок вызывает `spawn_agent` при умолчаниях | у ребёнка spawn-инструментов нет (`agents.max_depth` = 1), вызов → «unsupported call: spawn_agent» |
| s6 | `thread/unsubscribe` → `thread/resume` с тем же `config.agents` | роль работает после resume |
| s6 | resume без `config.agents` | spawn → «unknown agent_type 'plain'» — роли в треде не хранятся |
| s6 | v1, в аргументах spawn лишние `task_name`, `fork_turns` | принимаются — один набор аргументов годится для v1 и v2 |
| s4v2, s5b, s7 | v2: `config.features.multi_agent_v2=true` на треде или `-c 'features={ multi_agent_v2 = true }'` процесса | см. раздел 3 |
| s8, s9b | лимиты флагом процесса (`-c agents={ max_depth = 3, max_threads = 2 }` или `-c agents.max_depth=3 …`), роли вложенным объектом `config.agents` на треде | лимиты потеряны (вложенный spawn отклонён, третий ребёнок создан): объект `agents` треда **заменяет** `agents` из флагов `-c` |
| s9a | те же флаги без ролей на треде | лимиты действуют |
| s9d | лимиты повторены в объекте треда `config.agents = {A: …, max_depth: 3, max_threads: 2}` | роли и лимиты действуют |
| s9c | роли ключом с точкой `config."agents.A"` | сливается с флагами, но для имени `kit.qa` ключ неоднозначен — не годится |
| s9e | лимиты в `[agents]` файла `config.toml` (`CODEX_HOME`), роли на треде | лимиты действуют — со слоем файла объект треда сливается |
| все | уведомления дочерних тредов | идут в то же соединение со своим `threadId` (`turn/started`, `item/*`, `turn/completed`, `warning`); у родителя ровно один `turn/started` на каждый `turn/start`, сам по завершении ребёнка ход не начинается |

## 3. Ответы на вопросы

**1. Скиллы.** (а) Да, `config.skills.config=[{…, enabled:false}]` выключает скилл процесса на треде. (б) Да, ребёнок
наследует `skills.config` треда родителя. (в) Нет: `enabled=true` в файле роли не включает ничего — ни выключенное
тредом родителя, ни процессом, ни ролью вызывающего ребёнка; `enabled=false` выключает. Следствие: схема «тред видит
только свои, ребёнок — только скиллы цели» на 0.169.0 невозможна (выключенное тредом ребёнку не вернуть). Рабочая
схема: скиллы сессии и целей — в корнях процесса; **главный тред видит объединение** (как у claude: «visible to the
whole session»), **файл роли выключает скиллы объединения, которых нет у цели** — ребёнок первого уровня видит ровно
скиллы цели; вложенный ребёнок — пересечение скиллов ролей по цепочке. Селектор — `path = <каталог скилла>/SKILL.md`
(путь из конфига как есть, codex канонизирует): `name` — это имя из frontmatter, а у aae имя ссылки — ключ конфига
`skills.<имя>`, который с frontmatter никто не сверяет (`crates/aae-contract/src/config/load.rs:169` проверяет только
существование каталога).

**2. Multi-agent.** aae версию **не задаёт**: её выбирает codex по модели (`codex features list`: `multi_agent_v2` —
stable, по умолчанию выключен и включается метаданными модели; `multi_agent` (v1) — deprecated, включён). Роли работают
в обеих, инструкции роли доходят до ребёнка в читаемом виде в обеих:
- v1 (фейк по умолчанию): пространство `multi_agent_v1`, `spawn_agent{agent_type, message}` → сразу
  `{agent_id, nickname}`; ребёнок без истории, его второе developer-сообщение начинается с `developer_instructions`
  роли вместо родительских, задача — обычное user-сообщение.
- v2: пространство `collaboration`, `spawn_agent{task_name, message, agent_type, fork_turns}` (обязательны
  `task_name`, `message`) → `{task_name:"/root/<имя>"}`; по умолчанию (`fork_turns: all`) запрос ребёнка — история
  родителя, затем developer-сообщения «You are now a newly spawned child agent…», **отдельным сообщением инструкции
  роли**, текст команды, собственный список скиллов ребёнка; с `fork_turns:"none"` — как v1. Задача — элемент
  `agent_message` с `encrypted_content`, который фейк получает открытым текстом (утверждение эксперимента 9 «фейк его
  не читает» неверно). `model` и `model_reasoning_effort` роли применяются. У ребёнка v2 инструментов `collaboration`
  не было (и с `agents.max_depth=3`) — с неизвестной моделью вложенность v2 фейком не проверить.
Почему не задавать: v2 меняет формат запросов к провайдеру (`agent_message`), codex сам включает его только для
известных моделей; значение на треде пользователь не смог бы перекрыть через `native.codex`. Пользователь форсирует
`features.multi_agent_v2 = true` в `native.codex` (флаг процесса — проверено, s5c/s7). Фейком проверяемы обе версии:
v1 — без настроек, v2 — этим флагом в native-блоке теста.

**3. `config_file`.** Абсолютный путь работает и в `CODEX_HOME`, и вне его; каталог `CODEX_HOME/agents/` использовать
нельзя (предупреждение про `name`). Инлайном роль задать нельзя (ключи кроме `description`/`config_file` молча
игнорируются). Сам файл не обязателен и может быть пустым (тогда роль = имя + описание, ребёнок — копия родителя);
обязательных полей нет; `developer_instructions` пустым быть не может — ключ опускается. Файл читается на
`thread/start`, `thread/resume` и при создании каждого ребёнка, поэтому должен жить, пока жив тред, и не меняться.

**4. Resume.** Да: `thread/resume` с тем же `config.agents` возвращает роли; без него ролей нет. Адаптер и так шлёт
весь `config` заново — нужно только, чтобы файлы ролей существовали и перед resume.

## 4. Решения

1. **Process part (отпечаток):** `skills` = скиллы сессии **и целей** (объединение, сессия первая — одноимённый скилл
   из другого каталога уже ошибка загрузки). Больше ничего нового. Следствие существующего правила: цель со скиллами
   при `inherit_host` даёт процессу свой `CODEX_HOME` со ссылками на хостовый — как собственные скиллы сейчас.
2. **На тред (SessionPart):** роли — `config.agents.<тип> = {description, config_file}`. Роли в отпечаток не входят:
   агенты с разными целями и промптами по-прежнему делят один app-server; при `inherit_host` без скиллов `home`
   остаётся хостовым и в него ничего не пишется.
3. **Файлы ролей:** `<state>/codex/roles/<16 hex sha256 текста>.toml` — по содержимому, вне `CODEX_HOME`. Содержимое
   едет в `SessionPart` служебным ключом `roles: {<путь>: <текст TOML>}` (в отпечаток не входит, `split` остаётся без
   побочных эффектов); `start_session` и `resume_session` перед вызовом app-server пишут недостающие файлы и убирают
   ключ из параметров. Запись — под уже имеющимся замком `self.instances`, во временный файл и `rename`: файл,
   который codex другого треда читает при spawn, всегда целый. Файлы не удаляются (их столько, сколько разных ролей).
4. **Файл роли** (только ключи, которые codex применяет; любой другой роняет роль): `developer_instructions` =
   `system_prompt` цели (роль + фрагменты плагинов; если пуст или из пробелов — ключа нет, ребёнок остаётся с
   инструкциями сессии), `model`, `model_reasoning_effort`, `[[skills.config]] {path = "<каталог>/SKILL.md",
   enabled = false}` на каждый скилл процесса, которого у цели нет.
5. **`agents` из `native.codex`:** объект `config.agents` треда заменяет `agents` из флагов `-c` (s8/s9b), поэтому
   тред начинает `config.agents` с таблицы `native.agents` (если она есть) и добавляет роли — иначе пользовательские
   `agents.max_threads`/`max_depth` (решение 143 оставляет их пользователю) пропадут у любого агента с целями.
6. **Версия multi-agent и `skills.config` на треде — не задаются.** Встроенные типы (`default`, `explorer`, `worker`)
   и потолки (`agents.max_threads`: шесть открытых детей; v1 `agents.max_depth`: один уровень) — в документации.
7. Отступления от строки пакета в `.loop/PLAN.md` («тред включает только свои (allow-list)», «текущую версию
   multi-agent») записаны в `.loop/JOURNAL.md` (expert, цикл 127, РЕШЕНИЕ).

## 5. Шаги реализации

Оценка: ~80 строк продукта, ~110 строк тестов. `part.rs` сейчас 366 строк (предел файла 400) — роли в новом модуле;
`tests/harness/codex.rs` 360 строк — харнесс-тест в новом файле.

### Шаг 1. Фейк: `Reply::Call` — `crates/aae-testkit/src/openai.rs`

```rust
/// A call with these arguments of the request's function tool whose name ends with this (`spawn_agent`, in its
/// namespace if it has one); a text answer if the request has no such tool.
Call(String, Value),
```
Тело ветки `Reply::Mcp` переезжает в `Reply::Call(suffix, arguments)` с `"arguments": arguments.to_string()`;
`Reply::Mcp(suffix)` становится `Box::pin(answer(n, Reply::Call(suffix, json!({})), body)).await` (документация
варианта `Mcp` остаётся). Отдельного теста не нужно — покрывается харнесс-тестом шага 4.

### Шаг 2. Адаптер — `crates/aae-codex/src/roles.rs` (новый), `part.rs`, `lib.rs`

`roles.rs` (`mod roles;` в `lib.rs`), две функции:

```rust
/// The agent roles of the targets of `config`: `config.agents` of its thread, and the role files by path.
pub(crate) fn of(config: &EffectiveConfig, state: &Path)
    -> Result<(Map<String, Value>, Map<String, Value>), Failure>
/// The thread parameters of session part `part`, its role files written first.
pub(crate) fn write(part: &Value) -> Result<Value, Failure>
```
- `of`: первый `Map` начинается с `config.native.value["agents"]`, если это объект (решение 5); на каждую цель
  `(name, t)` из `config.targets` — таблица роли по решению 4 (`toml::to_string` от `toml::Table`/`json!`; ошибка →
  `config_error("subagent `{name}`: …")`), путь `state.join("codex/roles").join("<hash16>.toml")`, где hash —
  `Fingerprint::of("codex", &ProcessPart(json!(text))).0[..16]` (как в `part::account`); запись
  `{ "description": …, "config_file": <путь> }` в первый `Map`, `<путь> → <текст>` во второй. Скиллы процесса берутся
  из `part::skills(config)` (ниже); «у цели нет» = нет включённого скилла с таким именем в `t.skills`.
- `write`: клонирует `part`, снимает ключ `roles`; для каждого `(path, text)`, если файла нет: `create_dir_all`
  родителя, `fs::write(path.with_extension("tmp"), text)`, `fs::rename`. Ошибка ввода-вывода →
  `child::config_error("{path}: {e}")`.

`part.rs`:
- вынести расчёт скиллов из `process()` в `pub(crate) fn skills(config) -> BTreeMap<String, PathBuf>`: включённые
  скиллы с `path` сессии и затем целей (`std::iter::once(config).chain(config.targets.values())`,
  `entry().or_insert_with` — как в `crates/aae-claude/src/part.rs:24-31`); `process()` зовёт её. Комментарий и
  rustdoc `process`: «the session's skills and its targets'».
- `session(config, plan, state: &Path)`: после хуков `let (agents, files) = roles::of(config, state)?;` — непустой
  `agents` → `thread_config["agents"]`, непустой `files` → `params["roles"]`. Rustdoc: ключ `roles` — не параметр
  app-server, его снимает `roles::write`. Шапка модуля: роли — треда.

`lib.rs`:
- `split`: `part::session(config, plan, &self.state)`.
- `start_session`: `let params = { let _one = lock(&self.instances); roles::write(&session.0)? };` и
  `known.call("thread/start", params, None)`; `resume_session`: `let mut params = …` тем же способом вместо
  `part.0.clone()`. Замок не держится через `await`. `bind` по-прежнему читает исходный `part`.

### Шаг 3. Тест части — `crates/aae-codex/src/part/tests.rs`

Один тест `a_thread_gets_a_role_per_target_and_the_process_their_skills`: сессия со скиллом `own` (`/s/own`) и
`native.agents = { max_threads: 15 }`; цели `kit.qa` (description, `system_prompt` «Check.», model `gpt-role`, effort
`high`, скилл `qa` → `/s/qa`) и `bare` (только description). Проверить через `driver.split`:
- `process["skills"] == {own, qa}`;
- `s["config"]["agents"]["max_threads"] == 15`; `…["kit.qa"]["description"]`; `config_file` начинается с
  `/state/codex/roles/` и кончается `.toml`;
- `toml::from_str(s["roles"][config_file])` == `developer_instructions = "Check."`, `model = "gpt-role"`,
  `model_reasoning_effort = "high"`, `skills.config == [{path = "/s/own/SKILL.md", enabled = false}]`;
- файл роли `bare`: нет `developer_instructions`, выключены `own` и `qa`;
- `SessionPlan::Resume` даёт тот же `config.agents` и `roles`; конфиг без целей и без `native.agents` — ни `agents`,
  ни `roles` (одна строка в существующем тесте `the_process_gets_the_native_block_and_the_thread_the_rest`).

### Шаг 4. Харнесс-тест — `crates/aae-codex/tests/harness/subagents.rs` (новый, `mod subagents;` в `main.rs`)

Помощник `async fn role_reaches_the_child(v2: bool)` и два теста (`…_in_multi_agent_v1`, `…_v2`) на `common::Live`:
- план фейка: `vec![Reply::Turn(vec![Reply::Call("spawn_agent".into(), json!({ "agent_type": "kit.qa", "message":
  "Check it.", "task_name": "qa" })), Reply::Text("DONE".into())])]` — v1 принимает лишний `task_name`; ребёнок
  (v1 — без истории и без spawn-инструмента, v2 — с историей родителя, где уже есть `function_call_output`) получает
  текст;
- `Live::open(provider, dir, 0, "never", None, |c| …)`: два настоящих каталога скиллов с `SKILL.md` (маркеры в
  описаниях: `zq-own`, `zq-qa`); `c.skills["own"]`; цель `kit.qa` = клон `c` с `description` «Checks the zq-desc
  work.», `system_prompt` «ROLE-QA-zz», `model` `gpt-role`, скиллом `qa`; для v2 — `native["features"] =
  json!({ "multi_agent_v2": true })`;
- события дочернего треда приходят в тот же канал (`Live::ended` может поймать чужой `TurnEnded`), поэтому тест ждёт
  не событие, а запрос: опрашивает `live.provider.requests()` до запроса с `body["model"] == "gpt-role"` (срок 30 с);
- проверки: первый запрос (родитель) содержит `kit.qa` и `zq-desc` (тип в описании `agent_type`), `zq-own` и `zq-qa`
  (тред видит объединение); в запросе ребёнка есть developer-сообщение с `ROLE-QA-zz`, а его **последнее**
  developer-сообщение с `<skills_instructions>` содержит `zq-qa` и не содержит `zq-own` (в v2 раньше в истории лежит
  список родителя); затем `live.stop()`.
Тест пишет файлы ролей в `dir/state/codex/roles` — заодно проверяет `roles::write` на настоящем codex.

### Шаг 5. Документация (в том же коммите, что шаг 2; тексты — по-английски)

- `docs/design/config.md`, `### Subagents`: вместо «codex does not get the definitions yet.»:
  > codex: each target is an agent role of the thread (`config.agents.<type>`: the `description` and a generated role
  > file with the role prompt as `developer_instructions`, the model and the effort), given again on every resume. A
  > target with no role prompt keeps the session's instructions. The targets' skills are linked into the process's
  > `CODEX_HOME/skills` beside the session's, so the session sees them all; a role turns off the ones its target has
  > not, so a subagent sees its target's skills only, and one called by another subagent the skills both targets
  > have. codex's built-in types (`default`, `explorer`, `worker`) stay. codex picks its multi-agent version by the
  > model; `features.multi_agent_v2 = true` in `native.codex` forces the newer one. The limits are codex's, set in
  > `native.codex`: `agents.max_threads` (six open subagents by default) and, in the older version,
  > `agents.max_depth` (one level by default).

  Там же, строка ~291 («A codex agent with skills that inherits the host…»): «with skills, its own or its
  subagent targets',».
- `docs/design/contracts.md` 4.6, строка таблицы «Process and session», столбец codex: `ProcessPart` — «skills (the
  session's and its subagent targets', links in its own `CODEX_HOME/skills`; …)»; `SessionPart` — добавить «the agent
  roles of the subagent targets». В список «codex (`crates/aae-codex`)» — пункт:
  > - Subagent targets are agent roles of the thread: `config.agents.<type> = {description, config_file}`, over the
  >   `agents` table of the native block, which a thread's `agents` would replace. codex takes a role only from a
  >   file, so the adapter writes each (`developer_instructions`, `model`, `model_reasoning_effort`,
  >   `[[skills.config]]` turning off the process's skills the target has not; any other key drops the role) as
  >   `<state>/codex/roles/<hash of its content>.toml` before `thread/start` and `thread/resume`. Roles are not in
  >   the fingerprint: sessions with different targets share an app-server. The multi-agent version is codex's
  >   choice by the model. A child thread's notifications carry its own thread id and belong to no session
  >   ([experiments](experiments.md#codex-subagent-roles-experiment-10)).
- `docs/design/adapters.md:127-128`: «…claude takes them by `--agents`, codex as agent roles of the thread
  (`config.agents`, a generated role file per target).»
- `docs/design/experiments.md`: новый подраздел `### codex subagent roles (experiment 10)` после «codex roles and
  skills per thread», маркер и таблица из раздела 6 ниже. В таблице «codex compaction», строка «Compaction of a
  child thread»: последнее предложение заменить на «A v2 child was not checked.». В «Consequences», абзац «For codex
  native subagents»: дописать «A role's `[[skills.config]]` only disables: nothing a thread, the process or a calling
  role disabled comes back, so a per-thread allow-list would also bind the children.»
- Если варианты `Reply` перечислены в `docs/` (grep `Reply::Mcp`) — добавить `Call`.

### Готово, когда

`cargo nextest run -p aae-codex` (тест части и оба харнесс-теста) зелёный; `mise run ci` зелёный на закоммиченном HEAD.

## 6. Текст для `docs/design/experiments.md`

```markdown
### codex subagent roles (experiment 10)

Code: throwaway Python scripts in a temporary directory, not kept: a fake Responses API that logs every request and
answers a `CALL <tool> <json>` in the last user message with that tool call, and an app-server client (several
threads in one process, roles per thread as in experiment 9, three skills linked into `CODEX_HOME/skills`).

<!-- MEASURE id=codex-subagent-roles-exp10 date=2026-11-18 ver=codex-0.169.0 env=linux-arm64-orbstack src=.loop/cycles/127/notes/wp123-plan.md note="fake Responses API, unknown model names (fallback metadata), n=1 per case; nesting in v2 and the live models' default version not checked" -->

| Question | Answer |
|---|---|
| Selectors of `skills.config` | `name` is the `name` of the skill's front matter, not the name of its directory or link. `path` is the `SKILL.md` file: the link under `CODEX_HOME/skills`, the link's target and a path through another symlink and `..` all matched; the directory alone did not. |
| Disabling a skill per thread | `config.skills.config = [{path, enabled: false}]` hides it in that thread only, and in its children. |
| `[[skills.config]]` of a role file | `enabled = false` hides the skill in the child. `enabled = true` brings nothing back: not a skill the parent thread disabled, nor one the process disabled (`config.toml` or a `-c` flag), nor one the role of the calling child disabled. A child called by a child saw only what neither role disabled. |
| The role file | Only a file carries a role's settings: `developer_instructions` given in `config.agents.<role>` itself is ignored without a warning, and a role with an empty file or no `config_file` is the parent's copy under another name. An absolute path outside `CODEX_HOME` works. The file is read at `thread/start`, at `thread/resume` and for every child; a missing one drops the role with a warning ("Ignoring malformed agent role definition: agents.<role>.config_file must point to an existing file") and the thread starts. A file in `CODEX_HOME/agents/` is also read as a role of its own and warns "must define a non-empty `name`". `developer_instructions = ""` drops the role ("cannot be blank"). A role name with a dot (`kit.qa`) works. |
| Resume | `thread/resume` with the same `config.agents` gives the roles again; without it a spawn fails with "unknown agent_type". |
| `agents` of the process and of the thread | A thread's `config.agents` object replaces the `agents` table given by `-c` flags (`agents.max_depth`, `agents.max_threads` were lost) and merges with the one of `config.toml`; limits repeated inside the thread's object apply. |
| Multi-agent v1 (an unknown model's default) | Tools in the namespace `multi_agent_v1`; `spawn_agent{agent_type, message}` (it takes `task_name` and `fork_turns` too) returns `{agent_id, nickname}` at once. The child starts without history: its developer message begins with the role's `developer_instructions` in place of the parent's, the task is its user message. A child has no spawn tools unless `agents.max_depth` is over 1. Six children stayed open, the seventh was refused: "agent thread limit reached". |
| Multi-agent v2 (`features.multi_agent_v2 = true` on the thread or as a `-c` flag) | Tools in the namespace `collaboration`; `spawn_agent{task_name, message, agent_type, fork_turns}` returns `{task_name: "/root/<name>"}`. With `fork_turns` unset the child's request is the parent's history, then developer messages: "You are now a newly spawned child agent…", the role's `developer_instructions` alone, the team text, the child's own skill list; with `"none"` it is as in v1. The task is an `agent_message` item whose `encrypted_content` a fake provider gets as plain text. The role's model and effort apply. The child had no `collaboration` tools, with `agents.max_depth = 3` too. `codex features list`: `multi_agent_v2` stable and off by default, `multi_agent` deprecated and on. |
| Notifications of a child thread | They come on the same connection with the child's thread id (`turn/started`, `item/*`, `turn/completed`, `warning`); the end of a child started no turn of the parent. |
```

## 7. Не проверено

- Какую версию multi-agent берут живые модели на 0.169.0 (по сессии владельца — v2) и вложенность v2: у ребёнка v2 на
  фейке spawn-инструментов нет, причина не выяснена.
- Хуки и MCP ребёнка на 0.169.0 (наследование треда видели на 0.168.0 и для лимита компакции/хуков — в эксперименте 9).
- Что происходит с дочерними тредами, когда сессия закрывается (`thread/unsubscribe` родителя), пока ребёнок ещё
  работает: адаптер детей не закрывает. В v1 `spawn_agent` возвращается сразу, так что модель может закончить ход
  раньше ребёнка.
- Заменяет ли объект треда так же `mcp_servers`/`hooks`, заданные пользователем в `native.codex` (для `agents` —
  заменяет).
- Имя цели, совпадающее с ключом таблицы `agents` codex (`max_threads`, `max_depth`): роль перекроет лимит.

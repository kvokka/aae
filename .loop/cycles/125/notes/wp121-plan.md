# WP-121 · Субагенты: модель конфига и проверки — план реализации

Объём: ~200 строк продукта, ~250 строк тестов, ~40 строк документации. В 150k контекста помещается; делить не
нужно. Доставка в харнесс (WP-122/123), `GET /v1/agents` (WP-124), руководства (WP-127) — не трогать.

## 0. Ловушки (прочитать до кода)

- `scripts/core-boundary.sh` запрещает слова `claude` и `codex` в `aae-contract` вне тестов, в любом регистре, в
  комментариях тоже. Поэтому встроенные типы харнесса — поле `Adapter`, значения задают крейты адаптеров; в
  rustdoc поля имена харнессов не писать.
- `EffectiveConfig` собирается литералом в 7 местах тестов адаптеров (`grep -rn 'scope: Default::default()' crates`):
  после этой строки добавить `description: None,` и `subagents: None,` (один `sed`), плюс `EffectiveConfig::new`.
- `Adapter { .. }` собирается в 4 местах: `crates/aae-claude/src/adapter.rs`, `crates/aae-codex/src/adapter.rs`,
  два `static` в `crates/aae-contract/src/tests/config_launch.rs`.
- Лимит файла 400 строк: `compose.rs` 330, `assemble.rs` 331, `files.rs` 342 — всё новое уходит в новый
  `config/subagents.rs`; в старые файлы только строки, названные ниже.

## 1. Типы данных

| Где | Что |
|---|---|
| `config/files.rs`, `Settings` | `#[serde(skip)] pub description: Option<String>` и `#[serde(skip)] pub subagents: Option<Vec<String>>` (как `services`): ключ в `[settings]` оркестратора, проекта, rig'а сам даёт `unknown field`. Список — сырой, как написан. |
| `config/assemble.rs`, `EffectiveConfig` | `#[serde(default)] pub description: Option<Sourced<String>>`; `#[serde(default)] pub subagents: Option<Sourced<Vec<String>>>` — разрешённый список: `<pack>.<name>` агента или имя варианта, отсортирован, без повторов, сам агент внутри. `None` — ни один слой не задал список: только сам агент. |
| `driver/adapter.rs`, `Adapter` | `pub agent_types: &'static [&'static str]` — типы агентов, встроенные в харнесс. claude: `["general-purpose", "Explore", "Plan", "claude"]`; codex: `["default", "explorer", "worker"]`; тестовые `static` — те же. |

Отдельной карты в `Config` нет: список живёт в эффективном конфиге (его печатает `explain`, он попадает в снимок
прогона — это и нужно WP-122/123).

## 2. Разбор ключей — `config/variants.rs`

Новая функция, через неё идут все три слоя (агент, вариант, правка):

```rust
/// The settings of an agent's table; `description` and `subagents` are keys of an agent, a variant and an edit only.
fn agent_settings(mut table: toml::Table) -> Result<Settings, String>
```

- `description` — существующим `string(&mut table, "description")?`;
- `subagents` — `table.remove("subagents")`, `try_into::<Vec<String>>()`, ошибка
  `` `subagents` must be an array of strings ``;
- остальное — `toml::Value::Table(table).try_into::<Settings>()` с `retired(e.to_string(), "")`, затем вписать оба поля.

Вызовы: `agent_keys` (вместо своего `try_into::<Settings>()`; покрывает `agent.toml`, варианты и правки
оркестратора и проекта) и `rig_agents` (ветка `toml::Value::Table(table)` — вместо прямого `try_into::<Settings>()`).
`Loader::settings` поля не трогает — проходят как есть.

## 3. Слои — `config/assemble.rs`

- `apply`: в массив скаляров добавить `(&mut self.description, &settings.description)`; отдельно
  `if let Some(list) = &settings.subagents { self.subagents = Some(Sourced { value: list.clone(), from: from.clone() }) }`
  — сильнейший слой заменяет список целиком (слои идут от слабого к сильному, отдельной логики не надо).
- `explain`: `("description", &self.description)` в цикл к `effort`/`system_prompt`; после него
  `if let Some(s) = &self.subagents { line("subagents", json(&s.value), &s.from) }`.
  Строка вывода: `subagents = ["kit.implementer","kit.qa"]  # agent`. Нет слоя со списком — строки нет.

## 4. Сборка по имени — `config/compose.rs`

Сейчас вариант собирается только в `for_run_as`, а цель списка может быть вариантом. Вынести общий путь (заодно
убирает дублирование в `effective` и `for_run_as`):

```rust
/// The config of `name` in `p` at `r`: a variant's by its name, else the pack agent's `<pack>.<name>`, with `middle`
/// between the agent and the edits; its `subagents` resolved.
pub(super) fn named(&self, p: &Project, r: Option<&Rig>, name: &str, mut middle: Vec<(Layer, Settings)>)
    -> Result<EffectiveConfig, String>
```

1. `variant = p.variants.get(name).or_else(|| self.variants.get(name))`;
   `agent: Name = variant ? variants::base(p, v)? : reference(name)`; `a = agent_of(p, &agent)?`.
2. Вариант: `middle.push((Layer::Variant(name), v.settings.clone()))`, `assemble(.., &|key| key == name)`.
   Агент: `assemble` с прежним замыканием `named` (правка разрешается в этого агента).
3. `if let Some(list) = &mut config.subagents { list.value = subagents::resolve(self, p, r, &agent.pack, name, &list.value)? }`.

`effective` = `place` + `self.named(p, r, &agent.to_string(), Vec::new())`. Хвост `for_run_as` =
`let name = variant.unwrap_or_else(|| agent.to_string()); Ok((name.clone(), self.named(p, r, &name, middle)?))`.
`assemble` не меняется. Сообщение `no agent variant` из `for_run_as` уходит: в загруженном конфиге оно недостижимо
(`variants::check_project`).

## 5. Разрешение имён — новый `config/subagents.rs`

```rust
/// The agents and variants `list` names for `me`, an agent of pack `own`, at place `r` of `p`: `me` among them, sorted.
pub(super) fn resolve(config: &Config, p: &Project, r: Option<&Rig>, own: &str, me: &str, list: &[String])
    -> Result<Vec<String>, String>
```

Собирать в `BTreeSet<String>`, начиная с `me`. Для каждой записи `e`:

- **без `*`**: если `e` — видимый вариант (`p.variants`, иначе `config.variants`, и `variants::base(p, v).is_ok()`) →
  `e`. Иначе `check::resolve(&p.packs, own, &reference(e), has_agent, "agent")` — правило раздела «Names» (свой пак,
  затем единственный пак; `<pack>.<name>` — точно) → `mapped(found)`. Ошибка: ``format!("`subagents`: {e}")``, то есть
  `` `subagents`: no agent `nobody` `` или `` `subagents`: agent `qa` is ambiguous, write one of: … ``.
- **с `*`**: `pattern = reference(e)` (`kit.qa-*` → пак `kit`, имя `qa-*`; `qa-*` → пак пуст). Агенты всех паков, у
  которых `(pattern.pack.is_empty() || glob(&pattern.pack, pack)) && glob(&pattern.name, name)` → `mapped(..)`; при
  пустом паке ещё видимые варианты с `glob(&pattern.name, variant)`. Пусто — не ошибка.
- `mapped(a: Name)` = `r.and_then(|r| r.agents.get(&a)).cloned().unwrap_or_else(|| a.to_string())` — карта rig'а, как
  у шага формулы: на rig'е, который ставит вариант вместо агента, цель — вариант. Вариант, названный прямо, остаётся
  собой.

`own` — пак агента, у которого список (у варианта — пак базового агента), из какого бы слоя список ни пришёл.

```rust
/// `*` in `pattern` stands for any run of characters, an empty one too.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.strip_prefix(head).is_some_and(|rest| {
            (0..=rest.len()).filter(|i| rest.is_char_boundary(*i)).any(|i| glob(tail, &rest[i..]))
        }),
    }
}
```

Крейт под glob не добавлять.

## 6. Проверки при загрузке — `config/subagents.rs`, вызовы из `load.rs` и `compose.rs`

```rust
/// Why the session of `me` with config `c` at `r` cannot call a target of its list; one line per target.
pub(super) fn unfit(config: &Config, p: &Project, r: Option<&Rig>, me: &str, c: &EffectiveConfig) -> Vec<String>
```

Для каждой цели `t != me` из `c.subagents`: `bare` = часть после `.` (или имя варианта); конфиг цели —
`config.named(p, r, t, Vec::new())`, то есть цель одна, без слоёв пака и шага, на том же месте. Первая сработавшая
причина, сообщение ``subagent `<t>`: <why>``:

| Условие | `<why>` |
|---|---|
| `bare` есть в `agent_types` адаптера с `name == c.harness` (`config.adapters`) | `` `<bare>` is a built-in agent type of harness `<h>` `` |
| `named` вернул `Err(e)` | `e` как есть: `no layer sets \`harness\``, `plugin \`x\` is for codex, not for harness \`claude\`` (плагин цели не для харнесса — существующая проверка `assemble`) |
| `s.harness != c.harness` | `` it runs on harness `<s>`, the caller on `<c>` `` |
| `s.scope != c.scope` | `` its `scope` is not the caller's `` |
| `s.inherits_host() != c.inherits_host()` | `` its `inherit_host` is not the caller's `` |
| `s.description.is_none()` | `` it has no `description` `` |
| ключ `k` есть в `s.env` и в `c.env`, `value` разные | `` its `env.<k>` is not the caller's `` |

Ключ `env`, который есть только у одной стороны, — не ошибка. Собственный список цели не проверяется (вложенный
вызов идёт в том же наборе). Сам агент проверок не проходит: `description` ему не нужен.

```rust
/// Every agent and variant of every project, alone on every place: its list resolves and its targets fit.
pub(super) fn check(loader: &mut Loader, config: &Config)
```

- `if !loader.errors.is_empty() { return }` (как `check_runs`: ссылки должны быть разрешены).
- По проектам; файл ошибки — `p.dir.join("aae.toml")`. Имена: `<pack>.<name>` всех агентов всех паков плюс ключи
  `p.variants` и `config.variants`. На каждое имя — `seen: BTreeSet<String>` и цикл по `p.places()`:
  - `Ok(c)` → `unfit(config, p, Some(r), &name, &c)`;
  - ``Err(e) if e.starts_with("`subagents`")`` → `vec![e]`;
  - прочий `Err` — пропустить: агент один не собирается (нет харнесса, чужой вариант оркестратора), его шаги
    проверит `check_runs`.
  - на каждое `why`, которого ещё нет в `seen`: ``loader.fail(&file, format!("agent `{name}` on rig `{rig}`: {why}"))``
    — одна ошибка на причину, а не по одной на каждое место.
- Вызов в `Loader::run` (`load.rs`) сразу перед циклом `check_runs`: `super::subagents::check(&mut self, &config);`.

**«Другой harness на любой сборке шага»** — в `compose::check_runs`, ветка `Ok`: `for_run` уже собирает каждый
ai-шаг каждой формулы на каждом месте со слоями пака и шага, которые могут сменить `harness` вызывающего:

```rust
Ok((agent, e)) => {
    for why in super::subagents::unfit(config, p, p.place(rig), &agent, &e) {
        loader.fail(&file, format!("formula `{formula}` on rig `{rig}`: {why}"));
    }
    used.insert(e.harness);
}
```

Цель при этом — по-прежнему одна (`named` без `middle`), так что шаг с `harness = "codex"` у агента со списком из
claude-целей — ошибка этого шага. `check_runs` идёт только при пустом списке ошибок, поэтому дублей с `check` нет.
`mod subagents;` — в `config/mod.rs`.

## 7. Тесты — новый `crates/aae-contract/src/tests/config_subagents.rs` (+ строка в `tests/mod.rs`)

Фикстура: `BASE` (`kit.implementer`, claude) + агенты через `changes`, например
`("kit/agents/qa/agent.toml", "harness = \"claude\"\ndescription = \"Checks a change.\"\n")`; агенту вне шагов
`prompt.template.md` не нужен. Список вызывающего — дописать строку к `agent.toml` из `BASE`. Образцы: шаг с
launch-ключом — `config_launch.rs:96`; плагин с `harnesses` — `config_plugins.rs:197`; `ADAPTERS`, `HARNESSES` —
`config_launch.rs` (`pub(super)`).

| Тест | Что утверждает | Мутация, которая его краснит |
|---|---|---|
| `no_list_means_only_the_agent_itself` | `BASE`: `subagents == None`, в `explain` нет `subagents`; `subagents = []` → `["kit.implementer"]`, слой `Agent` | убрать `me` из начального набора |
| `a_name_is_bare_or_qualified` | `["qa", "other.docs"]` → оба; голое имя берёт свой пак раньше чужого | `own` → `""` |
| `a_pattern_matches_agents_and_variants` | `qa-*` (агенты двух паков и вариант), `kit.*` (только пак `kit`, без вариантов), `*-fast`, `q*a`; `zz-*` грузится и даёт только себя | `glob`: `None => true`; убрать ветку вариантов |
| `the_strongest_layer_replaces_the_list` | агент `["qa"]`, правка проекта `["docs"]` → без `qa`, слой `ProjectAgent`; правка rig'а — только на rig'е; вариант со своим списком через `for_run` → слой `Variant`, сам — имя варианта | в `apply` объединять списки |
| `a_rig_puts_its_variant_in_place_of_a_named_agent` | `agents = { qa = "qa-opus" }`: на `backend` — `qa-opus`, на корне — `kit.qa`; вариант, названный прямо, — везде | `mapped` → `a.to_string()` |
| `explain_prints_the_resolved_list` | строки `subagents = ["kit.implementer","kit.qa"]  # agent` и `description = "…"  # agent`; `description` из правки — слой правки | убрать строку из `explain` |
| `a_name_nobody_has_is_an_error` | ``agent `kit.implementer` on rig `root`: `subagents`: no agent `nobody` ``, и такая ошибка ровно одна | убрать `seen`; убрать `?` у `resolve` |
| `a_target_of_another_harness_is_an_error` | цель с `harness = "codex"`; отдельно: обе claude, шаг формулы ставит `harness = "codex"` → ``formula `kit.implement` on rig `backend`: subagent `kit.qa`: it runs on harness `claude` `` | убрать вызов `unfit` из `check_runs`; убрать сравнение харнессов |
| `a_target_of_another_scope_or_host_is_an_error` | `scope = "root"`; `inherit_host = false` у цели; у обоих `false` — грузится | убрать каждую строку таблицы |
| `an_env_key_with_two_values_is_an_error` | у цели `env = { LANG = "ru" }` против `LANG = "C"` оркестратора; ключ только у цели — грузится | `!=` → `==` |
| `a_target_plugin_for_another_harness_is_an_error` | цель вне шагов с плагином `harnesses = ["codex"]` | ветка `Err(e)` → пропуск |
| `a_built_in_agent_type_is_no_name_of_a_target` | `load_for_adapters(.., ADAPTERS)`: агент `Explore` в списке claude-агента — ошибка; агент `worker` в том же списке — грузится | убрать проверку |
| `a_target_needs_a_description` | цель без `description` — ошибка; правка `[agents.qa] description = ".."` её снимает; вызывающему `description` не нужен | убрать проверку |
| `the_keys_belong_to_an_agent` | `[settings] subagents = ["*"]` в `aae.toml` → `unknown field`; `subagents = "qa"` → `must be an array of strings` | убрать `serde(skip)` |

Мутации — руками: правка, `cargo nextest run -p aae-contract -E 'test(<имя>)'`, красный, откат; в отчёте цикла одна
строка на тест. Существующие тесты `config_launch`/`config_layers` должны остаться зелёными без правок (кроме двух
`static` адаптеров).

## 8. Документация

`docs/design/config.md`:

- **«Settings (any layer)»**, после таблицы — абзац: `description` (string, scalar) and `subagents` (array of strings,
  the strongest layer replaces the list whole) are keys of an agent only: `agent.toml`, a variant and an edit take
  them, a `[settings]` table does not (see "Subagents").
- **«Effective config»**: в абзац о слиянии — «`subagents` is replaced whole by the strongest layer that sets it»; в
  абзац про `explain` — строка `subagents = ["kit.implementer","kit.qa"]  # agent`: the resolved list, the agent
  itself in it; no line when no layer sets one.
- **«Names»**, третий пункт: в `subagents` ссылающийся пак — пак агента со списком (у варианта — базового агента);
  имя варианта пишется как есть.
- **«Agent»**: в перечень ключей — `description` и `subagents`; новый подраздел `### Subagents` (15–20 строк):
  что это (решение 143: нативный субагент харнесса, не прогон — без финишера, гейтов, `max_runs`, хуков
  оркестратора); формы записи (`qa`, `kit.qa`, имя варианта, `qa-*`, `kit.*`, `*`; `*` — любая, в том числе пустая,
  последовательность; голый шаблон берёт агентов всех паков и варианты, с паком — только агентов); сам агент всегда
  в списке, по умолчанию только он; карта `agents` rig'а; набор один на сессию — вложенный субагент зовёт в нём же,
  список цели не читается; конфиг цели — она одна на месте сессии (`Config::effective`), слои пака и шага вызывающего
  до неё не доходят; список ошибок загрузки из п. 6 (включая шаг формулы, меняющий `harness`); шаблон, поймавший
  неподходящего агента, — та же ошибка; встроенные типы — от адаптера. Доставка в харнесс здесь не описывается.
- **«Agent variants»**: текст верен (вариант берёт любые ключи `agent.toml`, кроме `prompt`) — не менять.

`docs/design/adapters.md` («Adding a harness»): строка `agent_types: &["<type>"],` в образец `ADAPTER` и одно
предложение: the agent types the harness has built in; an agent named like one cannot be a subagent on it.

## 9. Допущения — одной записью РЕШЕНИЕ в `.loop/JOURNAL.md`

1. `description` — слоистый скаляр (агент, вариант, правка), а не ключ только `agent.toml`: правка даёт описание
   агенту чужого пака без форка, и у цели оно читается из её эффективного конфига. Альтернатива (только `agent.toml`)
   требует отдельного поля и обхода «вариант → базовый агент».
2. Сам агент в списке всегда, но проверок п. 6 не проходит: иначе `description` стал бы обязателен каждому агенту и
   сломал все существующие конфиги. Что получит харнесс как определение самого агента — вопрос WP-122.
3. Шаблон строгий: совпал с несовместимым агентом (другой харнесс, `scope = "root"`) — ошибка загрузки, а не тихий
   пропуск. Буква пакета; `*` в смешанном проекте придётся сужать.
4. `explain` печатает `subagents` только когда слой задал список: иначе меняется вывод каждого агента.
5. `config explain --agent <вариант>` без формулы по-прежнему не работает — пакет этого не просит.
6. Встроенный тип сравнивается с именем без пака, с учётом регистра. Столкновение `kit.qa` и `other.qa` в одном
   списке не проверяется: какое имя уходит в харнесс, решает WP-122.
7. Опечатка в списке слабого слоя, перекрытого сильным, не ловится: проверяется действующий список.

## 10. Порядок и коммиты

1. Ключи, слои, `named`, `resolve`, `explain`, первые шесть тестов, config.md (ключи, имена, explain).
2. `Adapter::agent_types`, `unfit`, `check`, вызов в `check_runs`, остальные тесты, config.md (ошибки), adapters.md.
3. Точечно: `cargo nextest run -p aae-contract`, `cargo clippy --all-targets -q -- -D warnings` (затронуты `aae-claude`,
   `aae-codex`); гейт `scripts/loop/checks 125` — один раз на закоммиченном HEAD.

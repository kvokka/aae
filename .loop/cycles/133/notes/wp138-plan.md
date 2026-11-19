# WP-138 · Комментарии и родители бида в шаблонах промптов, TOON — план реализации

Автор: expert, цикл 133. Исполнитель: subcoder, одна сессия. Все решения приняты здесь; где написано «точно так» —
текст кода/документа берётся дословно. Идентификаторы, тексты ошибок и формулировки для `docs/` — по-английски.

## 0. Находки, которые coder должен знать до начала

1. **Гейт на HEAD красный.** `scripts/budget.sh` выходит с кодом 1:
   `crates/loop-manager-core/src/run/machine.rs has 401 lines, the limit is 400`. Строку добавил WP-136 (`832110b`:
   было 400, стало 401). Этот план чинит это по ходу (шаг 4 выносит `render_and_prepare` из `machine.rs`, файл
   становится ~382 строки). Запись НАХОДКА в журнал — текст в разделе 9.
2. **Готовый крейт TOON брать нельзя.** Проверены `toon-format` 0.7.0 (официальный, MIT, spec 4.4),
   `serde_toon` 0.3.0, `serde_toon_format` 0.2.0: все три включают `serde_json/preserve_order`. Из-за объединения
   фич cargo это меняет `serde_json::Map` с сортированного на «в порядке вставки» во всём workspace — порядок ключей
   всех JSON, которые продукт пишет и хранит (пример зависимости от порядка: ключ сервиса — хеш текста JSON,
   `crates/loop-manager-core/src/run/services.rs:150`). `etoon` 0.9.0 (Apache-2.0) тянет `sonic-rs`. `toon` 0.1.2 —
   старый формат и `regex`. Поэтому — своя запись, 119 строк кода (раздел 3.1), бюджет ядра позволяет
   (11 994 / 18 000 сейчас, после пакета ~12 200).
3. **Формат ушёл дальше, чем думали: spec 4.4.** Пустой массив — `key: []` (не `key[0]:`), строка с ведущим `#`
   обязана быть в кавычках (комментарий), есть «keyed tables» и «nested field groups». Наш кодировщик пишет простые
   формы (поля, инлайн-массивы, таблицы, списки) — это валидный TOON: 60 000 сгенерированных значений прошли
   туда-обратно через строгий декодер `toon-format` 0.7.0 без единого расхождения, ~94 % вывода побайтно совпало с
   официальным кодировщиком (остальное — те самые свёрнутые формы 4.x, которые мы не пишем, и лишние кавычки).
   Проверка сделана во временном проекте вне репозитория; в репозиторий крейт не добавляется даже как dev-зависимость
   (фичи объединяются и в тестовой сборке).
4. **br 0.7.6** (проверено во временной базе):
   - `br show <id> --json` уже отдаёт `comments: [{id, issue_id, author, text, created_at}]` (в порядке создания) и
     `parent: "<id>"`. Отдельный `br comments` адаптеру не нужен. Один вызов — 8 мс.
   - Комментарий меняет `updated_at` бида и пишет строку `commented` в `events` (это уже учтено адаптером).
   - `br delete <родитель>` убирает зависимость: у ребёнка пропадает `parent`, цепочка просто кончается.
   - `--json` сильнее переменных `BR_OUTPUT_FORMAT` / `TOON_DEFAULT_FORMAT`; с `BR_OUTPUT_FORMAT=toon` команда
     `br comments <id>` без флагов отвечает TOON. Это для WP-142 (окружение сессий пака), не для этого пакета.

## 1. Решения дизайна

| Вопрос | Решение | Почему |
|---|---|---|
| Имена в шаблоне | `{{ bead.comments }}` — комментарии бида прогона; `{{ bead.parents }}` — родители от корня вниз до непосредственного родителя, у каждого все поля `Bead` и его `comments` | одно имя на «поля и комментарии родителей»; корень первым — «большая картина, потом место задачи» |
| Где доступны | только в шаблоне промпта; в `if`, в `inputs` шага `formula`, в `where` — ошибка загрузки, как любое неизвестное поле | это текст для агента, не данные для условий; `Bead` (5.2) и события не меняются — комментарии не попадают в журнал событий |
| Объём | по умолчанию все; ключ ai-шага `last_comments = <n>` оставляет последние `n` комментариев бида и каждого родителя | пак `devloop` (WP-142) — до 50 раундов; отчёт+ревью одного цикла нынешнего лупа ~18 КБ, все комментарии в промпт не влезут |
| Формат | TOON, своя запись (`loop-manager-contract/src/toon.rs`) | находки 2 и 3 |
| Как значение попадает в шаблон | общее правило языка шаблонов: плейсхолдер внутри текста, который разрешился в объект или массив, пишется как TOON (раньше — ошибка «is an object or array, not text»). `bead.comments` и `bead.parents` — обычные массивы в контексте | один механизм вместо фильтров; заодно `{{ steps.<name>.data }}`, `{{ bead.labels }}`, объектные `{{ inputs.x }}` перестают ронять прогон и уходят в TOON |
| Когда читать | при каждом входе прогона в ai-шаг, шаблон которого называет `bead.comments` или `bead.parents`: эффект `lineage` в очереди `tracker:<project>`, прогон ждёт ответ в `preparing` (как claim), потом рендер | комментарий ревьюера, написанный в этом же прогоне, обязан попасть в промпт следующего раунда; снимок при claim устарел бы. Эффект в состоянии — рестарт его повторит, дедлайн `state` переармируется (`recover.rs:146-150`) |
| Где хранить | ответ целиком заменяет `runs.bead_data` (поля `Bead` + `comments` + `parents`) | новой колонки не нужно; `render.rs` берёт `bead` из `bead_data` как раньше; типизированно `bead_data` никто не читает |
| Интерфейс трекера | восьмая операция `lineage(id) -> Lineage` | ядро не знает br; операция добавляется, потому что появился вызывающий (contracts 5.1) |

Допущения этого плана (в `assumptions.md`, раздел 7.5) и что отложено (раздел 8).

## 2. Порядок работы и коммиты

Два коммита, сообщения по-английски:
1. `templates: an object or array placeholder inside text is TOON` — шаги 3.1–3.2 и их документы (7.1, абзац о TOON).
2. `WP-138: a prompt shows the comments and the parents of its bead` — всё остальное.

По ходу — только точечные проверки (раздел 10). `mise run ci` запускает coder на HEAD.

## 3. Крейт `loop-manager-contract`

### 3.1. Новый файл `crates/loop-manager-contract/src/toon.rs` — точно так

В `src/lib.rs` добавить `mod toon;` (приватный: снаружи крейта пока не нужен; `pub` сделает пакет про MCP).
В файле не должно быть слов `br`, `claude`, `codex` (`scripts/core-boundary.sh`).

```rust
//! TOON (Token-Oriented Object Notation, https://toonformat.dev) text of a JSON value: an agent reads structured data
//! in it for fewer tokens than in JSON (docs/design/contracts.md 2.3). An encoder only, of the plain forms of the
//! format (spec 4.4): fields, inline arrays, tables and lists, with the comma and two spaces a level. The keys of an
//! object come in the order of the value, which is sorted.

use serde_json::{Map, Value};

/// `value` as TOON, without a final newline.
pub(crate) fn encode(value: &Value) -> String {
    let mut lines = Vec::new();
    match value {
        Value::Object(fields) => object(&mut lines, fields, 0),
        Value::Array(items) => array(&mut lines, String::new(), items, 0),
        scalar => lines.push(primitive(scalar)),
    }
    lines.join("\n")
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn is_scalar(value: &Value) -> bool {
    !value.is_object() && !value.is_array()
}

fn row<'a>(values: impl Iterator<Item = &'a Value>) -> String {
    values.map(primitive).collect::<Vec<_>>().join(",")
}

/// The fields of an object, one per line at `depth`.
fn object(lines: &mut Vec<String>, fields: &Map<String, Value>, depth: usize) {
    for (name, value) in fields {
        field(lines, format!("{}{}", indent(depth), key(name)), value, depth);
    }
}

/// One field whose line starts with `head` (the indent or the `- ` of a list item, then the key); what it holds
/// goes one level below `depth`.
fn field(lines: &mut Vec<String>, head: String, value: &Value, depth: usize) {
    match value {
        Value::Object(fields) => {
            lines.push(format!("{head}:"));
            object(lines, fields, depth + 1);
        }
        Value::Array(items) => array(lines, head, items, depth),
        scalar => lines.push(format!("{head}: {}", primitive(scalar))),
    }
}

/// An array of a field or of the root: scalars on the line of its header, objects of the same scalar fields as a
/// table, anything else as a list.
fn array(lines: &mut Vec<String>, head: String, items: &[Value], depth: usize) {
    let columns: Vec<&String> = items
        .first()
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::keys)
        .collect();
    let same = |o: &Map<String, Value>| o.keys().eq(columns.iter().copied()) && o.values().all(is_scalar);
    if columns.is_empty() || !items.iter().all(|item| item.as_object().is_some_and(same)) {
        return list(lines, head, items, depth);
    }
    let names: Vec<String> = columns.iter().map(|name| key(name)).collect();
    lines.push(format!("{head}[{}]{{{}}}:", items.len(), names.join(",")));
    let rows = items.iter().filter_map(Value::as_object);
    lines.extend(rows.map(|o| format!("{}{}", indent(depth + 1), row(o.values()))));
}

/// An array without a table: empty, scalars on the line of its header, or a list of `- ` items one level below
/// `depth`.
fn list(lines: &mut Vec<String>, head: String, items: &[Value], depth: usize) {
    if items.is_empty() {
        return lines.push(match head.as_str() {
            "" => "[]".to_owned(),
            item if item.ends_with("- ") => format!("{item}[0]:"),
            field => format!("{field}: []"),
        });
    }
    if items.iter().all(is_scalar) {
        return lines.push(format!("{head}[{}]: {}", items.len(), row(items.iter())));
    }
    lines.push(format!("{head}[{}]:", items.len()));
    let dash = format!("{}- ", indent(depth + 1));
    for item in items {
        match item {
            Value::Object(fields) if fields.is_empty() => lines.push(dash.trim_end().to_owned()),
            // The first field stands on the line of the hyphen, the others below it, one level in.
            Value::Object(fields) => {
                for (at, (name, value)) in fields.iter().enumerate() {
                    let start = if at == 0 { dash.clone() } else { indent(depth + 2) };
                    field(lines, format!("{start}{}", key(name)), value, depth + 2);
                }
            }
            Value::Array(inner) => list(lines, dash.clone(), inner, depth + 1),
            scalar => lines.push(format!("{dash}{}", primitive(scalar))),
        }
    }
}

/// A key as it is when it is a plain name, quoted otherwise.
fn key(name: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    match name.chars().next() {
        Some(first) if (first.is_ascii_alphabetic() || first == '_') && name.chars().all(plain) => name.to_owned(),
        _ => quoted(name),
    }
}

fn primitive(value: &Value) -> String {
    match value {
        Value::String(text) => {
            // Quoted: what would read as another type, as structure, as a comment or as two values, and blank edges.
            let plain = !text.is_empty()
                && text.trim() == text
                && !matches!(text.as_str(), "true" | "false" | "null")
                && text.parse::<f64>().is_err()
                && !text.starts_with(['-', '#'])
                && !text.contains([':', '"', '\\', '[', ']', '{', '}', ','])
                && !text.contains(|c: char| c < ' ');
            if plain { text.clone() } else { quoted(text) }
        }
        // No exponent and no `-0`: the format's form of a number.
        Value::Number(n) if n.is_f64() => match n.as_f64() {
            Some(f) if f != 0.0 => f.to_string(),
            _ => "0".to_owned(),
        },
        other => other.to_string(),
    }
}

fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
```

### 3.2. `crates/loop-manager-contract/src/template.rs`

- `Unresolved` теряет поле `not_scalar` и помощник `absent`:
  ```rust
  /// A placeholder that cannot be substituted: its path is absent or null in the values it is resolved against.
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Unresolved {
      pub path: String,
  }
  ```
  `Display`: ``write!(f, "`{}` has no value", self.path)``.
- `Template::render` (док-комментарий: `/// Renders as text: a scalar as it is, an object or an array as TOON (contracts 2.3).`):
  ```rust
  Part::Path(path) => match lookup(context, path) {
      Some(Value::String(s)) => out.push_str(s),
      Some(v @ (Value::Number(_) | Value::Bool(_))) => out.push_str(&v.to_string()),
      Some(structured) => out.push_str(&crate::toon::encode(structured)),
      None => return Err(Unresolved { path: path.clone() }),
  },
  ```
- `Template::resolve`: `ok_or_else(|| Unresolved { path: path.clone() })`.
- Правило одно для промптов и для строк `inputs` (триггер, шаг `formula`): язык один (contracts 2.3).

### 3.3. `crates/loop-manager-contract/src/tracker.rs`

Добавить типы (после `Bead`) и операцию (после `create`); `Serialize` уже импортирован:

```rust
/// A comment on a bead (contracts 5.2).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Comment {
    pub author: String,
    pub created_at: String,
    pub text: String,
}

/// A bead with its comments, oldest first.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Commented {
    #[serde(flatten)]
    pub bead: Bead,
    pub comments: Vec<Comment>,
}

/// What a prompt shows of a bead beyond its fields (contracts 5.2): its comments, and its parents with theirs, from
/// the root down to its own parent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Lineage {
    #[serde(flatten)]
    pub bead: Commented,
    pub parents: Vec<Commented>,
}
```

```rust
    /// The bead `id` with its comments and its parents with theirs (contracts 5.1); a parent the tracker no longer
    /// has ends the line, `NotFound` with no `id`.
    fn lineage<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Lineage, TrackerError>>;
```

`json!(lineage)` даёт ровно то, что кладётся в `bead_data`: поля `Bead`, `comments`, `parents`.

### 3.4. `crates/loop-manager-contract/src/config/prompt.rs` (208 строк → ~222)

```rust
/// What only a prompt shows of the run's bead: its comments, and its parents with theirs (contracts 2.3).
const LINEAGE: [&str; 2] = ["bead.comments", "bead.parents"];

impl Prompt {
    /// Whether the template shows the comments or the parents of the run's bead: a run reads them as it enters
    /// the step (docs/design/contracts.md 5).
    pub fn shows_lineage(&self) -> bool {
        self.template.paths().any(|p| LINEAGE.contains(&p))
    }
}
```

В `project()`, цикл проверки шаблона против формулы (строки 57–70): путь из `LINEAGE` известен, когда у формулы
есть вход типа `task`; всё остальное — как было, через `path(..)`:

```rust
let task = formula.inputs.values().any(|i| i.kind == InputType::Task);
let known = |p: &str| (task && LINEAGE.contains(&p)) || path(p, &formula.inputs, &formula.steps[..at], false);
for bad in unknown.filter(|p| !known(p)) { /* тот же loader.fail */ }
```

`path()`, `step_inputs`, `step_if`, `schema.rs` не трогать: там `bead.comments` остаётся неизвестным полем
(`bead.comments.text`, `bead.comment`, `bead.parents.title` — тоже). Текст ошибки прежний:
`` `{{ bead.comment }}` is not a value of the runs of `kit.implement` ``.

### 3.5. Ключ шага `last_comments`

- `config/formula_file.rs`: в `StepFile::Ai` поле
  `/// The prompt shows the last comments only, this many (docs/design/config.md); absent, all.`
  `last_comments: Option<std::num::NonZeroUsize>,`
  и в `StepKind::Ai` поле `last_comments: Option<NonZeroUsize>` с док-комментарием
  `/// The most comments of the bead, and of each of its parents, the prompt shows: the last ones.`
  Ноль и отрицательное отвергает serde (ошибка загрузки без своего кода); у шагов других видов ключ — неизвестное
  поле (`deny_unknown_fields`).
- `config/items.rs` (397 строк, предел 400 — ровно две строки): в образец `StepFile::Ai { .. }` на строке 234
  добавить `last_comments,`, в `StepKind::Ai { .. }` на строке 249 — `last_comments,`. Больше там ничего не писать.
- Проверки «ключ есть, а шаблон комментариев не показывает» нет намеренно: метка `agent:` бида может дать шагу
  другого агента с другим шаблоном.

## 4. Крейт `loop-manager-core`

### 4.1. Новый файл `crates/loop-manager-core/src/run/lineage.rs` (в `run/mod.rs`: `mod lineage;`)

Шапка: `//! The prompt of the step a run enters (docs/design/contracts.md 2.3, 5): rendered at once, or, when it shows
the comments or the parents of the run's bead, once the tracker has given them as they are now. The read is an
effect on the queue of the project's tracker, and the run waits for it in `preparing`, as it does for its claim.`

Содержимое — три функции `impl RunHandler` и одна свободная:

```rust
/// Renders the prompt of the step `row` enters and asks the `run.preparing` hooks; a prompt that shows the comments
/// or the parents of the run's bead reads them first.
pub(super) fn render_and_prepare(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
    let shows = row.step_kind == "ai" && (self.config.prompt(&row.project, &row.agent)).is_ok_and(Prompt::shows_lineage);
    let Some(bead) = row.bead.as_ref().filter(|_| shows) else {
        return self.render_now(tx, row);
    };
    // The step and the attempt name this entry of the step: an answer to another one is dropped.
    let payload = json!({ "op": "tracker", "call": "lineage", "project": row.project, "bead": bead, "run": row.id,
        "step": row.step, "attempt": row.attempt });
    let dedupe = format!("lineage:{}:{}:{}", row.id, row.step, row.attempt);
    tx.effect(&tracker_key(&row.project), &dedupe, &payload)?;
    tx.set_deadline(&row.deadline("state"), tx.now_ms() + self.config.watchdog.start_timeout_ms)
}

/// (тело нынешней `machine.rs::render_and_prepare`, строки 40–58, перенесённое без изменений)
fn render_now(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> { .. }

/// The tracker answered the read of `payload`: the bead as it is now replaces the run's snapshot of it and the
/// prompt renders. An answer to another entry of a step, or to a run that prepares no more, is dropped.
pub(super) fn lineage(&mut self, tx: &mut Tx<'_>, payload: &Value, result: Result<Lineage, TrackerError>) -> Result<(), Error> {
    let row = Row::load(tx, payload["run"].as_str().unwrap_or_default())?;
    if row.state != "preparing" || payload["step"] != json!(row.step) || payload["attempt"] != json!(row.attempt) {
        return Ok(());
    }
    match result {
        Ok(lineage) => {
            tx.clear_deadline(&row.deadline("state"))?;
            tx.sql().execute("UPDATE runs SET bead_data = ?2 WHERE id = ?1", params![row.id, json!(lineage).to_string()])?;
            self.render_now(tx, Row::load(tx, &row.id)?)
        }
        // Reported and tried again by its queue; the run waits on, up to `start_timeout`.
        Err(e @ TrackerError::Unavailable { .. }) => self.tracked(tx, payload, Err(e)),
        Err(e) => self.terminal(tx, row, "failed", Some("infrastructure"), &format!("bead_unreadable: {e}")),
    }
}
```

```rust
/// Runs the read of the effect `payload` and reports it; `false` retries it (the tracker was unavailable).
pub(super) async fn read(tracker: &dyn Tracker, core: &CoreHandle<Msg>, payload: &Value) -> bool {
    let result = tracker.lineage(payload["bead"].as_str().unwrap_or_default()).await;
    let retry = matches!(result, Err(TrackerError::Unavailable { .. }));
    // A stopped writer drops the message; the effect is still queued and runs again at the next start.
    let _ = core.send(None, Msg::Lineage { payload: payload.clone(), result }).await;
    !retry
}
```

Пояснения:
- `self.tracked(tx, payload, Err(e))` — существующая ветка `(_, Err(Unavailable | Invalid))` в `beads.rs:202`: событие
  `tracker.sync_failed{op: "lineage", bead, error, next_retry_at}`. Новых событий нет.
- Повторный ответ (эффект выполнился дважды после `kill -9`) для того же входа, пока прогон ждёт хуки
  `run.preparing`, перерендерит промпт и спросит хуки заново — так же ведёт себя повторный ответ claim; состояние
  сходится.
- Отмена прогона во время ожидания: ответ отбрасывается проверкой `state != "preparing"`.
- Чтение — для любого прогона с бидом, в том числе для дочернего прогона шага `formula`, которому бид одолжен.

### 4.2. Правки существующих файлов ядра

- `run/machine.rs`: удалить `render_and_prepare` (строки 40–58) — она переехала. Вызовы (`machine.rs:36`,
  `beads.rs:180`, `finish.rs:262`) не меняются. Итог: файл ≤ 400 строк (ожидаемо 382). Убрать ставшие лишними `use`.
- `run/msg.rs`: вариант
  ```rust
  /// The bead of the read effect `payload` with its comments and parents, for the prompt that shows them.
  Lineage {
      payload: Value,
      result: Result<crate::tracker::Lineage, TrackerError>,
  },
  ```
- `run/mod.rs`: `mod lineage;` и в разборе сообщений рядом с `Msg::Tracked`:
  `Msg::Lineage { payload, result } => self.lineage(tx, &payload, result),`.
- `run/beads.rs` (383 строки — добавить ровно одну ветку в `execute`, перед `_ =>`):
  `"lineage" => return super::lineage::read(tracker.as_ref(), core, payload).await,`
- `run/render.rs`: ветка `(StepKind::Ai { timeout, last_comments, .. }, "ai") => (timeout, last_comments)`, связка
  `let (timeout, last_comments) = match ..`; перед `prompt.template.render(&context)`:
  ```rust
  if let Some(last) = last_comments {
      keep_last(&mut context["bead"], last.get());
  }
  ```
  и свободная функция:
  ```rust
  /// Keeps the last `n` comments of `bead` and of each of its parents (docs/design/config.md).
  fn keep_last(bead: &mut Value, n: usize) {
      if let Some(comments) = bead.get_mut("comments").and_then(Value::as_array_mut) {
          comments.drain(..comments.len().saturating_sub(n));
      }
      for parent in bead.get_mut("parents").and_then(Value::as_array_mut).into_iter().flatten() {
          keep_last(parent, n);
      }
  }
  ```
  (`get_mut`, не индекс: индекс по `null` превратил бы отсутствующий бид в объект.)
- Комментарии: `run/row.rs:35` → `/// The bead as the tracker last gave it: at the claim, or with its comments and
  parents for a prompt that shows them (contracts 5.2).`; `store.rs:41` — `` `bead` and `bead_data` are its bead as
  the tracker last gave it``; `run/recover.rs:146` → `// A run in `preparing` with a state deadline waits for the
  tracker: the claim of its bead, or its comments and parents.`

## 5. Адаптер `loop-manager-br` (`lib.rs` 393 строки → разгрузить)

- Новый файл `crates/loop-manager-br/src/parse.rs` (`//! What `br show --json` says of a bead
  (docs/design/tracker-br.md).`): перенести туда `field` и `bead` из `lib.rs:327-365` как `pub(crate)`, добавить:
  ```rust
  /// A bead of `br show --json` with its comments, oldest first, as br lists them.
  pub(crate) fn commented(v: &Value) -> Result<Commented, TrackerError> {
      let text = |c: &Value, k: &str| c[k].as_str().unwrap_or_default().to_owned();
      let comments = (v["comments"].as_array().into_iter().flatten())
          .map(|c| Comment { author: text(c, "author"), created_at: text(c, "created_at"), text: text(c, "text") })
          .collect();
      Ok(Commented { bead: bead(v)?, comments })
  }
  ```
  В `lib.rs`: `mod parse;` и `use parse::{bead, commented, field};`.
- `impl Tracker for Br`:
  ```rust
  fn lineage<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Lineage, TrackerError>> {
      Box::pin(async move {
          let mut line: Vec<Commented> = Vec::new();
          let mut next = Some(id.to_owned());
          // One `show` per bead, up the parents; a parent met twice would never end, one deleted meanwhile ends it.
          while let Some(id) = next.filter(|id| line.iter().all(|c| c.bead.id != *id)) {
              let shown = match self.br(READER, &["show", &id]).await {
                  Err(Fail(_, TrackerError::NotFound)) if !line.is_empty() => break,
                  shown => shown?,
              };
              let found = commented(shown.get(0).ok_or(TrackerError::NotFound)?)?;
              next = found.bead.epic.clone();
              line.push(found);
          }
          let mut line = line.into_iter();
          let bead = line.next().ok_or(TrackerError::NotFound)?;
          Ok(Lineage { bead, parents: line.rev().collect() })
      })
  }
  ```
  Актор — `READER` (`loop-manager`), как у остальных чтений демона. Итог `lib.rs` ~375 строк.

## 6. Тестовый набор и тесты

### 6.1. `crates/loop-manager-testkit/src/tracker.rs` (`MemoryTracker`, 274 → ~305)

- `State`: поле `/// The comments, oldest first, by bead id.` `comments: Vec<(String, Comment)>`.
- `/// Someone comments on `id`, as `br comments add` does.`
  `pub fn comment(&self, id: &str, author: &str, text: &str)` — `created_at = "2026-11-13T00:00:00Z"`, в журнал
  изменений не пишет.
- `lineage`: при `down` — `Unavailable { text: "down" }`; нет бида — `NotFound`; идти по `epic`, пока бид находится
  (удалённый `remove` родитель кончает цепочку); `Lineage { bead, parents }`, родители от корня.
- `crates/loop-manager-testkit/tests/core/epic.rs`, `impl Tracker for Chain`: делегировать
  `fn lineage<'a>(..) { self.0.lineage(id) }`. Других реализаций трейта нет (`Br`, `MemoryTracker`, `Chain`).

### 6.2. Тесты `loop-manager-contract`

Новый `src/tests/toon.rs` (+ `mod toon;` в `src/tests/mod.rs`), `use crate::toon::encode;`. Ключи в ожиданиях —
в алфавитном порядке (так хранит `serde_json::Value`). Ожидаемые строки — точно так:

1. `comments_are_a_table_and_a_bead_with_parents_a_list`:
   - `[{"author":"alice","created_at":"2026-11-19T03:59:18Z","text":"go left"},{"author":"loop-manager:run_1:2","created_at":"2026-11-19T04:10:00Z","text":"Tests: 2 red, see below\n- a::b"}]` →
     ```
     [2]{author,created_at,text}:
       alice,"2026-11-19T03:59:18Z",go left
       "loop-manager:run_1:2","2026-11-19T04:10:00Z","Tests: 2 red, see below\n- a::b"
     ```
   - `{"comments":[],"id":"t-1","labels":["auto","ui"],"parents":[{"comments":[{"author":"bob","text":"why"}],"id":"t-0","labels":[],"title":"Epic: all of it"}],"priority":2}` →
     ```
     comments: []
     id: t-1
     labels[2]: auto,ui
     parents[1]:
       - comments[1]{author,text}:
           bob,why
         id: t-0
         labels: []
         title: "Epic: all of it"
     priority: 2
     ```
   - `{"exit_code":1,"stderr":"","stdout":"FAILED 3 tests\n"}` → `exit_code: 1` / `stderr: ""` / `stdout: "FAILED 3 tests\n"`.
2. `a_string_is_quoted_when_it_would_not_read_back_as_itself`:
   `["plain words",""," pad","true","12","-x","# head","a: b","a,b","[x]","q\"q","tab\t","\u{1}"]` →
   `[13]: plain words,""," pad","true","12","-x","# head","a: b","a,b","[x]","q\"q","tab\t","\u0001"`
   (в Rust-литерале ожидания обратные слэши экранировать: `\\t`, `\\u0001`, `\\\"`).
3. `mixed_arrays_are_lists_and_odd_keys_are_quoted`:
   - `[1,{"a":1},[2,3],[],{},{"b":{"c":null},"d":-0.0,"e":1.5}]` →
     ```
     [6]:
       - 1
       - a: 1
       - [2]: 2,3
       - [0]:
       -
       - b:
           c: null
         d: 0
         e: 1.5
     ```
   - `{"key with space":1,"9":true,"a.b":{"c":[]}}` → `"9": true` / `a.b:` / `  c: []` / `"key with space": 1`.
   - корень: `json!("a: b")` → `"a: b"` (с кавычками), `json!([])` → `[]`, `json!({})` → пустая строка.

`src/tests/template.rs`:
- три места с `not_scalar` → `Unresolved { path: "…".into() }`;
- в `a_prompt_template_substitutes_scalars_and_fails_on_missing_values` вместо «an object is not text»:
  `{{ rig }}` рендерится в `path: /src`; добавить контексту `"labels": ["auto", "ui"]` в `bead` и проверить
  `"Labels {{ bead.labels }}"` → `Labels [2]: auto,ui`; переименовать тест в
  `a_prompt_template_substitutes_scalars_writes_structures_as_toon_and_fails_on_missing_values`;
- в `inputs_keep_json_types_…` добавить ключ `"all": "labels {{ event.bead.labels }}"` → `"labels [2]: auto,ui"`.

`src/tests/config_prompt.rs` (133 строки), два теста:
- `a_prompt_shows_the_comments_and_the_parents_of_its_bead_and_no_field_below_them`:
  `prompt(&[(OWN, "{{ bead.comments }}\n{{ bead.parents }}")]).shows_lineage()` истинно, у `prompt(&[])` ложно;
  для `bead.comment`, `bead.comments.text`, `bead.parents.title` — `fails(.., "`{{ <path> }}` is not a value of the
  runs of `kit.implement`")`; формула без входа `task` (как `review` в соседнем тесте) с шаблоном
  `{{ bead.comments }}` — та же ошибка для `kit.review`.
- `last_comments_is_a_positive_count_on_an_ai_step`: формула с `last_comments = 2` грузится и
  `StepKind::Ai { last_comments, .. }` равен 2; `last_comments = 0` — ошибка загрузки; `last_comments = 2` на шаге
  `exec` — ошибка загрузки (неизвестное поле).

`src/tests/config_if.rs`: одна проверка — `if = { "bead.comments" = "x" }` → ошибка
`` `if`: a run has no `bead.comments` ``.

### 6.3. Тесты ядра: новый модуль `crates/loop-manager-testkit/tests/core/bead_comments.rs` (+ в `tests/core/main.rs`)

Образцы: `tests/core/beads.rs` (`world(FIX)`, `slow_turn()`, `w.fix(json!(bead))`, `w.finish_attempt(..)`,
`w.driver.sent(&run, n).1`), внешний трекер — `w.tracker`. Все три — `#[tokio::test(start_paused = true)]`, без
exec-шагов. Файлы мира: агент `fixer` с шаблоном
`"Fix {{ bead.title }}.\nComments:\n{{ bead.comments }}\nParents:\n{{ bead.parents }}\n{{ remarks }}"`, агент `qa` с
шаблоном `"Review {{ bead.title }}."`, формула `fix`: вход `bead` типа `task`, шаги `code` (ai, `fixer`) и
`review` (ai, `qa`, `back_to = "code"`).

1. `a_prompt_shows_the_comments_of_the_bead_and_its_parents_from_the_root_down_as_toon`: биды `root`,
   `mid` (в `root`), `bead` (в `mid`); комментарии: `root` — `alice`: `The goal: ship it`; `bead` — `bob`:
   `start with the parser`, потом `carol`: `two lines\nhere`. Промпт первой попытки содержит
   `"[2]{author,created_at,text}:\n  bob,\"2026-11-13T00:00:00Z\",start with the parser\n  carol,\"2026-11-13T00:00:00Z\",\"two lines\\nhere\"\n"`,
   строку `      alice,"2026-11-13T00:00:00Z","The goal: ship it"`, и `    id: <root>` стоит раньше `    id: <mid>`;
   бид без комментариев у `mid` виден как `comments: []`.
2. `a_return_to_a_step_reads_the_comments_written_since_and_last_comments_keeps_the_last`: шаг `code` с
   `last_comments = 1`. Первая попытка: в промпте `Comments:\n[]`. Завершить `code` (`Pass`), на шаге `review`
   добавить два комментария (`qa`: `first pass`, затем `qa`: `redo: the parser drops the last line`), завершить
   `review` (`Fail`). Промпт третьей попытки (новый вход в `code`) содержит
   `[1]{author,created_at,text}:\n  qa,"2026-11-13T00:00:00Z","redo: the parser drops the last line"` и не содержит
   `first pass`.
3. `a_run_waiting_for_the_comments_survives_a_restart_and_fails_once_the_tracker_never_answers`:
   - прогон на шаге `review`; `w.tracker.set_down(true)`; завершить `review` (`Fail`); дождаться, пока прогон
     войдёт в `preparing` шага `code` (ход `slow_turn` кончается через 10 с после промпта — ждать событие
     `run.step_finished` через `w.until(..)` или ≥ 12 с остановленных часов); есть `tracker.sync_failed` с
     `op == "lineage"` и строкой `next_retry_at`; `w.restart().await`; добавить комментарий; заскриптовать ход новому
     драйверу (`w.driver.script(slow_turn())` — после рестарта драйвер другой); `set_down(false)`; через 10 с
     третья попытка получила промпт с этим комментарием;
   - второй прогон на другом биде: то же до входа в `preparing` при лежащем трекере, потом 121 с простоя
     (`start_timeout` — 120 с) — `run.failed` с `reason == "tracker_unavailable"`.

Общее для трёх тестов: ход скриптуется на каждую попытку заранее (`w.driver.script(slow_turn())`), номера попыток —
1 (`code`), 2 (`review`), 3 (`code` второго раунда). Ожидаемые строки в пунктах 1–2 даны как текст промпта: в
Rust-литерале кавычки и обратные слэши экранируются (в промпте `"two lines\nhere"` — это кавычка, `\`, `n`).

### 6.4. Тест адаптера: `crates/loop-manager-br/tests/contract.rs` (306 строк)

`the_lineage_of_a_bead_is_its_comments_and_its_parents_from_the_root_down`: `root` (epic), `epic` (epic,
`--parent root`), `kid` (`--parent epic`); `br comments add <root> "The goal: ship it"` и два комментария на `kid`
(через помощник `br(..)`, актор `alice`). `t.lineage(&kid)`: `bead.bead.id == kid`, `bead.comments` — два, в порядке
добавления, `author == "alice"`, тексты совпадают, `created_at` не пуст; `parents` по id — `[root, epic]`, у `root`
один комментарий, у `epic` ни одного. `t.lineage("t-none")` → `Err(NotFound)`. Обновить шапку файла (перечень
того, что проверяется).

## 7. Документы (английский; истории и номеров пакетов в `docs/` нет)

### 7.1. `docs/design/contracts.md`

- 2.2, строка 181: `(2.3, without `remarks` and `agent`)` → `(2.3, without `remarks`, `agent` and the bead's
  `comments` and `parents`)`.
- 2.3, пункт про типы (строки 191–192) заменить на: `A string that is exactly one placeholder takes the JSON type of
  the value (an integer stays an integer, an object stays an object). A placeholder inside a longer string is
  rendered as text: a scalar as it is, an object or an array as TOON (below).`
- 2.3, пункт про корни промпта: после `` `bead` (when the run has one)`` вставить в конец пункта: `` `bead` holds the
  fields of `Bead` (5.2) and, in a prompt template only, `bead.comments` — the comments of the bead, oldest first,
  each `{author, created_at, text}` — and `bead.parents` — its parents from the root down to its own parent, each
  with the fields of `Bead` and its own `comments`; a path below either is a config error. A run reads both from
  the tracker (`lineage`, 5.1) each time it enters an `ai` step whose template names one of them, so the prompt
  holds what was written up to then, by the earlier steps of the run too, and every `bead.*` of that step is the
  bead as read then. `last_comments` of the step keeps the last of them (config.md, "Formula").``
- 2.3, новый абзац после списка: `Structured data in a prompt is TOON ([toonformat.dev](https://toonformat.dev),
  spec 4.4), not JSON: an agent reads it for fewer tokens. loop-manager writes the plain forms of the format, with
  the keys of an object in alphabetical order: `key: value` fields, nested by two spaces; an array of scalars on one
  line (`labels[2]: auto,ui`); an array of objects that have the same scalar fields as a table, a header
  `[2]{author,created_at,text}:` and one row per object; any other array as a list of `- ` items; an empty array as
  `[]`. A string is quoted when it would not read back as itself: empty, blank at an edge, a number, `true`, `false`
  or `null`, starting with `-` or `#`, or holding `:`, `,`, `"`, `\`, a bracket, a brace or a control character. It
  does not write the keyed tables and the nested field groups of the spec; a TOON decoder reads its output all the
  same.`
- 5.1: `seven operations` → `eight operations`; в блок трейта строку
  `fn lineage(&self, id: &str) -> BoxFuture<Result<Lineage, TrackerError>>;          // comments and parents`;
  новый абзац после абзаца про `under`: `` `lineage` gives the bead `id` with its comments, oldest first, and its
  parents with theirs, from the root down to its own parent: what a prompt shows of a bead beyond its fields (2.3).
  It is a read, made as `loop-manager`. A parent the tracker no longer has ends the line; an `id` it does not have
  is `NotFound`. The core asks for it by an effect on the queue `tracker:<project>` when a run enters an `ai` step
  whose prompt template names `bead.comments` or `bead.parents`, and the run waits for the answer in `preparing`, as
  for its claim: the answer replaces the run's snapshot of the bead and the prompt renders; an `Unavailable` tracker
  is tried again (`tracker.sync_failed{op: "lineage"}`) until `start_timeout` fails the run
  (`tracker_unavailable`), any other error fails it at once (`failed{infrastructure, bead_unreadable: <error>}`).
  An answer to another entry of a step, or to a run that prepares no more, is dropped.``
- 5.2: новый пункт после `Bead`: `` `Comment`: `{author, created_at, text}`. `Lineage`: the fields of `Bead`,
  `comments: [Comment]` and `parents`, each the fields of `Bead` and its `comments`. Comments are no part of `Bead`:
  a bead event carries none.``
- 6.1, строка `preparing` (679): `the bead claim confirmed, the comments and parents of the bead read when the
  step's prompt shows them, `run.preparing` hooks running, …`; колонка таймаута: `` `start_timeout` for the claim
  and for that read, then the hook deadline (see 3.3)``.
- 6.2, строка 761: в причины добавить `` `lineage` error other than `Unavailable` `` и `no claim, `create` or
  `lineage` answer within `start_timeout``; в исходы — `` `failed{infrastructure, bead_unreadable: <error>}` ``.
  Строка 781: `the next step's prompt renders (after the read of the bead's comments and parents when it shows
  them; the workdir and the bead claim stay)`.
- 6.4, строка 882: `` `decide` (3.3), the claim of a bead and the read of its comments and parents in `preparing`
  (`start_timeout`)``.

### 7.2. `docs/design/config.md`

- Блок формулы, в ai-шаг после `native.claude`:
  `last_comments = 6                   # optional, ai steps only: the prompt shows the last 6 comments of the bead`
  `                                    # and of each of its parents (default: all)`
- Абзац про `formula`-шаг (строка 756), после `is the parent's snapshot of it`: `(a step of the child whose prompt
  shows the comments or the parents of the bead reads them itself)`.
- Абзац «A step sees the outcomes…» (829–838), после `stdout, stderr}`)`: `; a placeholder that holds an object or
  an array, `{{ steps.<name>.data }}` as a whole among them, is written as TOON (contracts 2.3)`.
- Абзац проверки шаблона (860–863): `` `bead.<field>` needs a `task` input and a field of `Bead` (contracts 5.2),
  or `comments` or `parents`, which only a prompt template may name;``
- Новый абзац после него: `` A prompt shows what was said on the task: `{{ bead.comments }}` is the comments of the
  run's bead, `{{ bead.parents }}` its parents from the root down, each with its fields and its comments — the
  whole picture and the place of the task in it. Both are TOON (contracts 2.3), so a template puts each on a line
  of its own. The run reads them from the tracker each time it enters the step: a comment an earlier step of the
  same run left, or a human meanwhile, is in the prompt of the next step and of the next round. `last_comments`
  on the step keeps the last comments of the bead and of each parent, and the agent reads the older ones itself
  (`br comments <id>`). A bead with no comments shows `[]`. ``
  и пример:
  ````
  ```
  [2]{author,created_at,text}:
    alice,"2026-11-19T03:59:18Z",go left
    "loop-manager:run_7:2","2026-11-19T04:10:00Z","Tests: 2 red\n- parser::tail"
  ```
  ````

### 7.3. `docs/design/tracker-br.md`

- «Beads and runs», новый пункт после пункта про `formula`-шаг (строка 42): `A prompt that shows the comments or the
  parents of the run's bead (`{{ bead.comments }}`, `{{ bead.parents }}`, [contracts.md 2.3](contracts.md#23-substitution-inputs-and-prompt-templates))
  gets them from `br show <id> --json`, one call for the bead and one for each parent up to the root: its `comments`
  (`author`, `created_at`, `text`, in br's order, oldest first) and its `parent`. The run reads them each time it
  enters such a step, so the comments that agents and humans leave on a bead are how the steps and rounds of a run,
  and the runs after it, hand the work over.`
- «Limits of br the adapter is shaped by», новый пункт: `` `br delete` of a parent removes the link: its children
  have no parent then, and the line of parents a prompt shows ends there. A comment changes the bead's `updated_at`
  and reaches the core as `bead.updated` with `changed: ["comments"]`, without its text.``

### 7.4. `docs/agents/decisions.md` — раздел «Agents, packs, formulas», новая строка (только слова владельца)

`| 148 | An agent sees the comments of its task in its prompt and, where the formula needs them, the comments and the fields of the task's parents: the whole picture and the place of the task in it. Data that goes into an agent's context is given as TOON, not JSON (fewer tokens); wherever else data reaches a context, TOON is preferred where that is reasonable (2026-11-19) |`

### 7.5. `docs/agents/assumptions.md` — новые пункты

Раздел «Agents, packs, formulas»:
- `` A prompt names the comments of the run's bead `bead.comments` and its parents `bead.parents`, the root first, each parent with every field of `Bead` and its comments; nothing but a prompt template may name them. All comments are shown unless the `ai` step has `last_comments`, which keeps the last of the bead and of each parent.``
- `` The bead is read again each time a run enters an `ai` step whose prompt shows its comments or parents; the other steps see the bead as it was last read.``
- `` TOON is written by loop-manager's own encoder, the plain forms of spec 4.4 only: the crates that write TOON turn on `preserve_order` of `serde_json` for the whole build, which would change the key order of every JSON loop-manager writes. An object or array placeholder inside any template text is TOON, in a trigger's `inputs` too.``
- `` What a script reads stays JSON: `LOOP_MANAGER_STEP_<NAME>_DATA`, `LOOP_MANAGER_INPUT_<NAME>`, `--json` of the CLI. The answers of the MCP tools are still JSON.``

### 7.6. `docs/guide/first-formula.md:194`

После `and so on,` вставить: `` the comments on it as `{{ bead.comments }}` and its parents as `{{ bead.parents }}`
([config](../design/config.md#formula-formulasfiletoml)),`` (якорь сверить с тем, как на «Formula» ссылаются соседние
строки гайда).

## 8. Другие места, где данные уходят в контекст агента

| Место | Сейчас | Что делаем |
|---|---|---|
| `{{ remarks }}` и дописка замечания к промпту | текст: `<check>: <summary>` и хвост вывода (`run/finish.rs:313`) | не JSON, ничего |
| `{{ steps.<name>.data }}` целиком, объектный `.data.<key>` | прогон падал: `is an object or array, not text` | **этот пакет**: TOON общим правилом |
| `{{ bead.labels }}`, объектные и списочные `{{ inputs.<name> }}` | то же падение | **этот пакет**: TOON общим правилом |
| строки `inputs` триггера и шага `formula` с объектом внутри текста | срабатывание пропускалось (`input_unresolved`) | **этот пакет**: TOON (язык один) |
| `{{ bead }}`, `{{ run }}`, `{{ rig }}`, `{{ project }}`, `{{ inputs }}` целиком | ошибка загрузки | не трогаем: никто не просил |
| ответы MCP-инструментов (`crates/loop-manager-front/src/api/mcp.rs:162`, `value.to_string()`) | JSON текстом | **следующий пакет** (текст ниже): одна строка кода, но переписать разбор ответа в `crates/loop-manager/tests/e2e/mcp.rs:43,142`, сценарий `c17_mcp.rs`, `docs/api/mcp.md` |
| `br` из сессии агента | JSON или текст, как попросит агент | **WP-142**: `env.BR_OUTPUT_FORMAT = "toon"` в окружении сессий пака (находка 4) |
| `--json` у CLI `loop-manager`, который агент зовёт сам | JSON | не трогаем: его читают и скрипты; флаг формата — отдельный вопрос владельцу, если понадобится |
| `LOOP_MANAGER_STEP_<NAME>_DATA`, `LOOP_MANAGER_INPUT_<NAME>`, `LOOP_MANAGER_ADDON_PARAMS` | JSON в окружении | остаётся JSON: читает скрипт, не модель |
| `prompt_append` хуков, заметки `context-budget`, роль, описания субагентов | текст | ничего |

Предлагаемый пакет в `.loop/PLAN.md` (номер — следующий свободный, после WP-142 или перед ним — на усмотрение
coder; WP-142 от него не зависит):

> ### WP-14x · Ответы MCP-инструментов в TOON · размер S · статус: todo
> - Решение 148: данные в контекст агента — TOON. Ответ инструмента MCP (`start_run`, `run_state`, `list_runs`,
>   `cancel_run`, …) сейчас — JSON текстом (`crates/loop-manager-front/src/api/mcp.rs:162`).
> - Сделать: текст ответа — `toon::encode` (сделать `loop_manager_contract::toon` публичным); ошибки — как есть;
>   тесты e2e и сценарий 17 сверяют текст, а не разбирают JSON; `docs/api/mcp.md`.
> - Готово, когда: тесты MCP зелёные на TOON-ответах; `docs/api/mcp.md` совпадает с кодом; `mise run ci` зелёный.

## 9. Записи в `.loop/JOURNAL.md` (делает coder; тексты)

- **НАХОДКА**: «`scripts/budget.sh` на HEAD (`8f367c1`) падает: `crates/loop-manager-core/src/run/machine.rs` — 401
  строка при пределе 400; строку добавил WP-136 (`832110b`). То есть `mise run ci` был красным два пакета. WP-138
  выносит `render_and_prepare` из файла (382 строки).»
- **РЕШЕНИЕ**: «TOON пишет свой кодировщик (`loop-manager-contract/src/toon.rs`, 119 строк), а не крейт: `toon-format`
  0.7.0, `serde_toon` 0.3.0 и `serde_toon_format` 0.2.0 включают `serde_json/preserve_order`, что поменяло бы порядок
  ключей всех JSON продукта. Кодировщик пишет простые формы spec 4.4; проверен вне репозитория: 60 000
  сгенерированных значений прочитаны обратно строгим декодером `toon-format` 0.7.0 без расхождений. Новые
  допущения — `assumptions.md`: имена `bead.comments` / `bead.parents`, чтение при входе в шаг, `last_comments`,
  общий TOON для объектов в шаблонах.»
- **РЕШЕНИЕ**: «Ответы MCP-инструментов остаются JSON в WP-138 (размер M); перевод на TOON — отдельный пакет
  (раздел 8 плана). Окружению сессий `devloop` в WP-142 — `BR_OUTPUT_FORMAT=toon`.»

## 10. Точечные проверки по ходу (вывод обрезать)

```bash
cargo nextest run -p loop-manager-contract -E 'test(toon) | test(template) | test(prompt) | test(config_if)' --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -40
cargo nextest run -p loop-manager-testkit --test core -E 'test(bead_comments) | test(beads) | test(steps) | test(returns)' --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -40
cargo nextest run -p loop-manager-br --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -40
LOOP_MANAGER_CHAOS_SEEDS=96 cargo nextest run -p loop-manager-testkit --test chaos 2>&1 | tail -20
cargo clippy --workspace --all-targets -q --message-format short -- -D warnings 2>&1 | head -60
cargo fmt --all --check; scripts/budget.sh | tail -12; scripts/core-boundary.sh; scripts/unused-pub.sh
```

Каждая команда — с лимитом времени инструмента. Acceptance, демо, живые модели не запускать; `demos/` и `devloop/`
не трогать (шаблоны `devloop` перепишет WP-142).

## 11. Ловушки

- Пределы файлов: `config/items.rs` 397 (+2), `run/beads.rs` 383 (+1), `run/machine.rs` 401 (−19),
  `br/src/lib.rs` 393 (вынести `parse.rs`), `tests/config_steps.rs` 379 (новые тесты шагов — в `config_prompt.rs`).
- `scripts/core-boundary.sh`: в коде ядра вне тестов (в комментариях тоже) нет слов `br`, `claude`, `codex`. В
  `lineage.rs`, `toon.rs`, `tracker.rs` писать «the tracker».
- `unreachable_pub`: `toon` — приватный модуль, `encode` — `pub(crate)`. `scripts/unused-pub.sh`: каждое `pub` имя
  используется где-то ещё (`Comment`, `Commented`, `Lineage`, `shows_lineage`, `MemoryTracker::comment` — да).
- В `tests/core/` нельзя `start_paused` вместе с реальным процессом — в новых тестах только ai-шаги.
- Тесты адаптера `br` запускают `br` из `PATH`; не находится — `mise exec -- cargo nextest ..`.
- `StepKind::Ai { .. }` с полным перечнем полей есть только в `config/items.rs:249`.
- `Unresolved { path, not_scalar }` вне `template.rs` встречается только в `src/tests/template.rs`.

## 12. Готово, когда

- тесты разделов 6.2–6.4 зелёные: рендер шаблона с комментариями и родителями в TOON; неизвестное поле
  (`bead.comment`, `bead.comments.text`) — ошибка загрузки; новый раунд видит новые комментарии; ожидание чтения
  переживает рестарт;
- `contracts.md`, `config.md`, `tracker-br.md`, `assumptions.md`, `first-formula.md` совпадают с кодом; решение 148
  в `decisions.md`;
- `scripts/budget.sh` зелёный (в том числе `machine.rs` ≤ 400); `mise run ci` зелёный на HEAD (запускает coder).

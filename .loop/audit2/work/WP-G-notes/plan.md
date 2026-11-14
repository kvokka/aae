# WP-G — события драйвера до потребителей: план

Основание: решение владельца, спека §2.2 п. 5 [Р]: «То, что адаптер сообщает (`usage` с лимитом аккаунта, `retrying`,
причина конца хода, ухудшение здоровья, порядковый номер), ядро читает и отдаёт политике и наблюдателю; не удаляется».
Порядок работ: 1 → 2 → 3 → 6 → 4 → 5, по одному коммиту на пункт (код + тест + документы пункта вместе).

## 0. Общее для всех пунктов

**Что уже есть и переиспользуется**

- CLI печатает ответ API как есть (`crates/aae-front/src/cli/mod.rs: print`), поэтому новое поле представления
  появляется в `aae run show` / `aae instance list` само. Править нужно только рукописную схему ответа.
- Новый тип события и новое необязательное поле `data` разрешены contracts 1.1 («Adding an optional field to a
  payload or a new event type keeps `schema`»). Там же: «The daemon never emits a field it does not document here» —
  каждое новое поле обязано появиться в contracts 1.3.
- `resync.diverged{subject: "instance", id, expected, observed}` уже есть в contracts 1.3 и в `config/schema.rs`.

**Места, которые ломаются молча, если их забыть**

| Что | Где | Чем ловится |
|---|---|---|
| Формы `data` для `where`/шаблонов | `crates/aae-core/src/config/schema.rs: payload()` | ничем: триггер на новое поле будет отвергнут загрузчиком |
| Рукописные схемы ответов | `crates/aae-front/src/api/replies.rs` (`Run`, `Attempt`, `Instance`) | `crates/aae/tests/replies.rs` (+ `tests/common/schema.rs: fits` — лишний ключ ответа = провал) |
| Закоммиченный OpenAPI | `docs/api/openapi.json` | `crates/aae-front/tests/openapi.rs`; перегенерация: `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi` |
| Счётчики тестов приёмки | `crates/aae-acceptance/src/checks.rs`: критерий 11 — ровно 36 тестов в `binary(contract)`, критерий 17 — ровно 7 | новые тесты WP-G кладём в `aae-testkit`, в `contract.rs`/`openapi.rs`/`replies.rs` тестов не добавляем |

**Схема sqlite** (`crates/aae-core/src/store.rs`, список миграций в `open`): по одной строке на коммит, по образцу
`(13, "ALTER TABLE attempts ADD COLUMN waited …")`. Нужны: п. 1 — (15), п. 4 — (16), п. 5 — (17). Новые столбцы
дешевле JSON-поля: в коде уже принято `UPDATE … SET col = col + ?`.

**Новый файл тестов стенда**: `crates/aae-testkit/tests/driver_events.rs` (пп. 1, 3, 4, 5; `instances.rs` уже 357
строк, туда не дописывать). Образец мира со встроенной политикой и аккаунтом `a` — `tests/auth.rs: world(params)`.
Тест п. 2 — в `tests/watchdog.rs` (там есть `stall_at` и конфиг `WATCHDOG`).

**Запас по размеру файлов (предел 400)**: `run/events.rs` 327, `run/machine.rs` 363, `run/registry.rs` 321,
`run/exec.rs` 306. Поэтому помощники кладём: учёт токенов и лимита — в `run/row.rs` (140) и `run/gates.rs` (206),
проверку `seq` и здоровье — в `run/registry.rs`.

---

## 1. `Usage` и `RateLimit` — размер M

**Факты.** Оба адаптера шлют приращения: claude — `usage` из `result` (за ход; `stream.rs:170`), codex —
`tokenUsage.last` (`stream.rs:162`). Событие с лимитом несёт нули токенов (`claude stream.rs:251`, `codex
stream.rs:328`). Ключ аккаунта уже хранится: `runs.account` = `account:<harness>/<hash>`, пишется в
`machine.rs: start_attempt` через `learn_keys` до запуска (и из `failure.account`).

**Схема (миграция 15)**

```sql
ALTER TABLE attempts ADD COLUMN input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE attempts ADD COLUMN output_tokens INTEGER NOT NULL DEFAULT 0;
CREATE TABLE account_limits (account TEXT PRIMARY KEY, utilization REAL NOT NULL, resets_at_ms INTEGER,
    at_ms INTEGER NOT NULL);
```

Сумма по прогону — запросом `SUM` по `attempts`, отдельного столбца на `runs` не надо. Таблица лимитов — строка на
аккаунт, не растёт.

**Ядро**

- `run/events.rs: attempt_event` — до `match`, рядом с `if let Normalized::PromptAcked` (то есть в любом живом
  состоянии, не только `running`): на `Normalized::Usage{..}` вызвать `Self::usage(tx, &row, attempt, in, out,
  rate_limit)`. Дальше событие идёт в общую ветку тишины, как сейчас.
- `run/gates.rs` (ключ аккаунта живёт здесь):
  - `usage(tx, row, attempt, input, output, limit: Option<&RateLimit>)` — `UPDATE attempts SET input_tokens =
    input_tokens + ?, output_tokens = output_tokens + ?`; при `Some(limit)` — `INSERT OR REPLACE INTO account_limits
    … SELECT account, … FROM runs WHERE id = ? AND account IS NOT NULL`.
  - `limit_of(tx, row) -> Value` — `{utilization, resets_at, at}` (времена через `rfc3339_ms`) или `null`.
- `run/row.rs`:
  - `attempts()` — два поля в элемент: `input_tokens`, `output_tokens`.
  - `usage(conn, run) -> Value` — `{input_tokens, output_tokens}`, `COALESCE(SUM(..), 0)` по всем попыткам.
- `run/machine.rs: attempt_failed` — в `data` ключ `"rate_limit": Self::limit_of(tx, &row)?`.
- `run/machine.rs: terminal` — во все три события (`run.passed`, `run.failed`, `run.cancelled`) ключ `"usage"`.
- `run/read.rs: Reader::run` — `view["usage"]`.
- `config/schema.rs` — `("usage", Obj[input_tokens, output_tokens])` в три терминальных события;
  `("rate_limit", Obj[utilization, resets_at, at])` в `run.attempt_failed`.

**Политика** (`crates/aae-plugins/src/policy.rs: attempt_failed`)

```rust
// The account's last known limit, if it says exhausted and has not reset yet: its reset time.
let known = Some(&data["rate_limit"])
    .filter(|l| l["utilization"].as_f64().is_some_and(|u| u >= 1.0))
    .and_then(|l| time(&l["resets_at"]))
    .filter(|t| *t > now);
```

- `quota_exhausted`: `retry_at` сбоя, иначе `known`, плюс `quota_margin_s`; ни того ни другого — как сейчас,
  `now + quota_wait_s`.
- `rate_limited` при `known.is_some()` и наличии ключа `account:` — `park` на воротах аккаунта до `known + margin`
  (ветка перед общей `"rate_limited" | "overloaded"`). `overloaded` не трогаем.

Допущение (в журнал): порог `utilization >= 1.0`. Без него прогон с окном на 41 % парковался бы до недельного
сброса вместо 30 минут. Если харнесс не дал `utilization` (0), поведение остаётся прежним — регрессии нет.

**Фронт**: `replies.rs` — `Run.usage: Usage` (новая структура `{input_tokens: u64, output_tokens: u64}`),
`Attempt.input_tokens`, `Attempt.output_tokens`; перегенерировать OpenAPI.

**Тесты стенда** (`driver_events.rs`)

1. `the_tokens_of_a_run_add_up_over_its_events` — мир без политики; сценарий `[TurnStarted, Usage{100, 20, None},
   Usage{5, 7, None}]` + `hollow_turn()`. Проверить: `run.failed.data.usage == {105, 27}`; `Reader::run` —
   `usage` и `attempts[0].input_tokens == 105`. Без правки поля нет.
2. `a_quota_without_a_reset_time_waits_for_the_accounts_known_reset` — мир с политикой и аккаунтом `a`. Прогон 1:
   `Usage{0, 0, Some(RateLimit{utilization: 1.0, resets_at_ms: Some(7_200_000)})}` и обычный пустой ход (политика
   напомнит и завершит). Прогон 2 на том же аккаунте, без своих `Usage`: `Failure(QuotaExhausted)` без `retry_at`.
   Проверить: `run.attempt_failed.data.rate_limit.resets_at == "1970-01-01T02:00:00.000Z"`,
   `gate.closed.gate.until == "1970-01-01T02:01:00.000Z"`. Без правки `until` = «сейчас + 30 мин».

Плюс модульные случаи в `crates/aae-plugins/src/tests.rs` (помощник `failed` получает `rate_limit`): исчерпан и
впереди → сброс + запас; 0.4 → 30 мин; сброс в прошлом → 30 мин; `rate_limited` с исчерпанным лимитом → ворота
аккаунта.

**Документы**: contracts 1.3 (`RunView` для API — `usage`; `Attempts` — два поля; три терминальных события;
`run.attempt_failed` — `rate_limit`), 4.3 (строка `usage`: приращения токенов копятся на попытке, лимит хранится по
аккаунту и уходит в `run.attempt_failed`), 10 (строка `quota_exhausted`, новая оговорка для `rate_limited`);
`docs/design/config.md:493` (`quota_wait_s` — «нет ни времени сброса, ни известного лимита»); `adapters.md` —
`Usage` несёт приращения.

**Риски контрактных тестов адаптеров**

- Единицы: оба адаптера переводят `resetsAt` из секунд в мс (`claude stream.rs:240`, `codex stream.rs:330`); у codex
  то же поле уже проверено критерием 10 через `retry_at`. `utilization` — доля 0..1 у обоих (codex делит
  `usedPercent` на 100). Код адаптеров не меняется.
- [НП] claude: `result.usage` считаю расходом за ход, а не накопленным за процесс. Слабое подтверждение — в
  лаб-логе `claude/logs/exp7-native-529x2ok.ndjson` два `result` подряд несут одинаковые 10/3. Если накопленный —
  попытки с замечаниями завысят сумму. Проверить существующим сценарием с двумя ходами или записать в журнал.
- claude не включает в `input_tokens` кэш-токены (`cache_read_input_tokens`, `cache_creation_input_tokens`):
  расход на входе занижен. Не расширяем, оговорка в `adapters.md`.
- codex шлёт снимок лимита только тредам с открытым ходом (`stream.rs: self.open()`): снимок между ходами до ядра
  не дойдёт. Регрессии нет.

---

## 2. `Retrying{attempt, delay, text}` — размер S

**Решение по событию.** `run.state_changed` не годится: состояние не меняется (`running → running`), у события
нет полей под номер повтора и паузу, а триггеры на `to == "running"` сработали бы на каждый повтор. Берём новое
наблюдаемое событие `run.retrying` — contracts 1.1 это разрешает, прецедент — `trigger.switched`. Назначение из
спеки: §5.4 «молчаливых ожиданий нет» — раз сторож теперь терпит объявленную паузу, причина должна быть в журнале.

**Ядро** (`run/events.rs: attempt_event`, новая ветка после охраны `_ if row.state != "running"`):

```rust
Normalized::Retrying { attempt: retry, delay, text } => {
    let delay = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
    row.emit(tx, "run.retrying", json!({ "attempt": row.attempt, "generation": row.attempt, "retry": retry,
        "next_retry_at": rfc3339_ms(tx.now_ms().saturating_add(delay)), "text": text }))?;
    silence(tx, &row, w.silence_ms.saturating_add(delay))
}
```

В замыкании `silence` заменить `tx.now_ms() + ms` на `saturating_add` (пауза приходит от харнесса). Потолок паузы
не нужен: дедлайн шага (`state` → `runaway_turn`) ограничивает ход целиком. Имя `next_retry_at` — как в
`tracker.sync_failed`. `config/schema.rs` — строка `run.retrying`.

**Тест** (`tests/watchdog.rs`, `silence = 1m`): `stall_at(vec![Emit(TurnStarted), Emit(Retrying{attempt: 1, delay:
90 s, text: "overloaded"})])` == `"1970-01-01T00:02:30.000Z"` (без правки — `00:01:00`); в журнале `run.retrying`
с `retry == 1` и `next_retry_at == "1970-01-01T00:01:30.000Z"`.

**Документы**: contracts 1.3 (новая строка наблюдаемых событий), 4.3 (строка `retrying`), 6.1 (строка `running`:
объявленная харнессом пауза прибавляется к пределу тишины); `invariants.md` строка 15.

**Риск**: у codex `delay` всегда 0 (`stream.rs:146`) — предел тишины для него прежний, 20 с паузы по `Retry-After`
в него укладываются. У claude событий до 10 за ход — шум журнала ограничен.

---

## 3. `TurnEnded.stop_reason` — размер S

**Ядро** (`run/events.rs`), без миграции — причина едет в существующем `attempts.turn_ended`:

- ветка `Normalized::TurnEnded { stop_reason }`: писать код причины вместо `1` (1 — completed, 2 — cancelled,
  3 — failed);
- `turn_settled`: читать `turn_ended` числом (0 — хода не было), в `data` класть
  `["completed", "cancelled", "failed"][code - 1]` вместо константы на строке 216.

Реакцию политики не меняем: спека её не называет.

**Тест** (`driver_events.rs`): `the_event_of_a_turn_without_an_outcome_tells_how_the_turn_ended` — сценарий
`[TurnStarted, TurnEnded{Cancelled}, State{Idle, false}]` → `run.turn_ended_without_outcome.data.stop_reason ==
"cancelled"`; `hollow_turn()` → `"completed"`.

**Документы**: contracts 1.3 (значения `stop_reason`), 4.3 (строка `turn.ended`).

**Что реально дойдёт**: `failed` — никогда (адаптер шлёт `Failure` раньше, попытка уже закрыта, `TurnEnded`
отбрасывается как поздний). `cancelled` — только отмена не от ядра (codex `turn.status == "interrupted"`): своя
отмена идёт в эффекте `stop` уже после конца попытки.

---

## 6. `PromptState::Answered / Unanswered` — размер S

**Где сливаются**: `run/exec.rs:226`, ветка `Transcript::Present { .. } => {}` — оба случая шлют промпт сверки
(`RECONCILE`, `machine.rs:19`) с новым ключом. Это записано в contracts 6.4, то есть слияние намеренное.

**Почему «Unanswered — ждать» невозможно.** К моменту resume в сессии нет идущего хода по построению:

- после смерти демона `recover.rs:17-26` останавливает каждый записанный экземпляр; переподключения в первом
  выпуске нет (§5.6 п. 2 [Д]);
- при любом другом сбое `end_attempt` ставит эффект `stop` (`cancel_turn` + `close_session`), а запуск ждёт смерти
  старого процесса (`old_instances`, инвариант 6).

Ждать нечего — прогон упал бы по `stall`. «Answered — ход состоялся» тоже не развести одинаково: у claude это
«после промпта есть запись ассистента» (`lib.rs:356`, ответ мог оборваться), у codex — «ход завершён» (`lib.rs:334`).

**Решение: свести к `Transcript::Present { delivered: bool }`.** Это единственное удаление в пакете. В перечне
владельца (§2.2 п. 5) этого поля нет, а потребитель различия появится только с переподключением к живым
экземплярам. Правки: `driver.rs` (убрать `PromptState`), `exec.rs:214`, `aae-claude/src/lib.rs:354-358`,
`aae-codex/src/lib.rs:332-336`, `aae-testkit/src/lib.rs` (`Session.prompts` → `Vec<PromptKey>`, в `play.rs: emit`
остаётся только `in_turn = false`), проверки в `aae-claude/src/tests.rs`, `aae-claude/tests/claude.rs:115`,
`aae-codex/tests/codex.rs:121,131`, `aae-testkit/src/tests.rs`. Обе ветки уже покрыты `tests/recovery.rs:168-194`.

**Запасной вариант**, если ревьюер сочтёт, что «не удаляется» покрывает и это поле (размер M): найденное состояние
уходит наблюдателю — столбец `attempts.transcript`, поле `transcript: "absent"|"unanswered"|"answered"` в элементе
`Attempts` у попыток `resume`. Поведение не меняется.

**Документы**: contracts 4.2 (`Transcript`), 6.4 (последний пункт); `adapters.md` («Known limits», про
`transcript(key)`).

**6b, находка рядом (S, отдельным коммитом или записью в журнал).** `run/prompts.rs: unsettled` смотрит только на
попытки со сбоем `daemon_crash`. После любого другого сбоя (например `stall` в `starting`, промпт потерян) resume
проверяет новый ключ, получает `Absent`, но `retry` пуст — и сессия получает только `RECONCILE`, без задачи. Для
claude это прикрыто (`Missing` → `transcript_divergence` → новая сессия); для codex — [НП]. Правка: убрать условие
`json_extract(a.failure, '$.kind') = 'daemon_crash'`. Тест в `recovery.rs`: `lose_prompts(true)`, дождаться `stall`
по `start_timeout`, решение `Resume` → тексты `["Build web.", "Build web."]` с одним ключом. Причина фильтра в
истории (`ad3156a`) не записана — на настоящем харнессе не проверял.

---

## 4. `Health::Degraded{text}` — размер M

**Факт.** Ни один адаптер его не создаёт: оба зовут `driver/child.rs: health` (жив ли pid + время старта → `Ok`
или `Dead`). В шпаргалках нет сигнала уровня экземпляра, который отображался бы в «ухудшение». Производитель
сегодня — только стенд. Потребитель всё равно нужен: §2.2 п. 5 называет ухудшение здоровья прямо.

**Схема (миграция 16)**: `ALTER TABLE instances ADD COLUMN degraded TEXT;`

**Ядро**

- `run/exec.rs: watch` (опрос раз в 30 с уже есть): держать `last: Option<String>`; на `Health::Degraded{text}` и
  на возврат к `Ok` — только при смене значения — слать `Msg::InstanceHealth { id, degraded: Option<String> }`.
  `CoreHandle` взять из `self.core.get().cloned()`.
- `run/msg.rs` — вариант `InstanceHealth`; `run/mod.rs: handle` — ветка.
- `run/registry.rs: instance_health(tx, id, degraded)` — `UPDATE instances SET degraded = ?2 WHERE id = ?1 AND
  state = 'ready'`; если строка изменилась: `Some(text)` → `instance.degraded {instance, reason: text}`, `None` →
  повторный `instance.ready {instance, reason: null}`. Состояние экземпляра не трогаем, сессии не роняем, из
  `acquire` не исключаем.
- `run/read.rs: instances` — поле `degraded`. `config/schema.rs` — `instance.degraded` в строку к
  `instance.starting | ready | stopped`.

**Фронт**: `replies.rs: Instance.degraded: Option<String>`, перегенерировать OpenAPI.

**Стенд**: `aae-testkit/src/lib.rs: Instance` — поле `degraded: Option<String>`, `health()` отдаёт `Degraded`;
`control.rs: degrade(&self, instance: &str, text: Option<&str>)`.

**Тест** (`driver_events.rs`): `a_degraded_instance_is_told_and_keeps_its_sessions` — прогон с долгим ходом,
`degrade(id, Some("…"))`, 30 с виртуального времени → `instance.degraded` с `reason`, `Reader::instances()[0]
.degraded`, состояние `ready`, нет `run.attempt_failed`; `degrade(id, None)` → второй `instance.ready`, поле `null`.

**Документы**: contracts 1.3 (`instance.degraded`; `instance.ready` приходит снова после восстановления), 4.2,
8 (пункт Health и поле записи); `adapters.md`.

---

## 5. `DriverEvent.seq` — размер S

**Возможен ли пропуск.** Нет, по построению: `child::Events::send` → один неограниченный `mpsc` в процессе
(`run/mod.rs:68`) → одна задача-пересыльщик → очередь писателя. Канал не теряет и не дублирует. Повтор так же
недостижим из адаптеров. Оба случая воспроизводит только стенд. Но проверка монотонности — одно сравнение, и сигнал
у обоих случаев один, поэтому делаем одну проверку, а не две.

**Схема (миграция 17)**: `ALTER TABLE instances ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;` Столбец, а не поле
в памяти: запись экземпляра удаляется при его конце, и поздние события ушедшего экземпляра не дают ложных сигналов.

**Ядро**

- `run/registry.rs: in_order(tx, instance, seq) -> Result<bool, Error>`: записи нет или `seq == 0` → `true`;
  `seq != last + 1` → `resync.diverged {subject: "instance", id, expected: last + 1, observed: seq}`;
  `seq <= last` → `false` (событие отброшено, ноль эффектов); иначе записать `seq`, `true`.
- `run/events.rs: driver_event` — первой строкой: `if !Self::in_order(tx, &event.instance, event.seq)? { return
  Ok(()) }`.

**`seq = 0` от самого ядра** (`registry.rs:246` — resync нашёл процесс мёртвым; `exec.rs:298` — то же по опросу
здоровья): это `Exited`, который сделало ядро, вне нумерации адаптера. Оставляем 0 как «не нумеровано», проверку
пропускаем, фиксируем в комментарии к `DriverEvent.seq` и в contracts 4.3.

**Стенд**: `step.rs` — `Step::Reseq(u64)` («следующее событие экземпляра выйдет с этим номером»), `play.rs` —
ветка `i.seq = n - 1`.

**Тест** (`driver_events.rs`): `a_repeated_event_is_dropped_and_a_gap_is_told` — `[Emit(TurnStarted) /* seq 2 */,
Reseq(2), Emit(Failure(HarnessCrash)), Reseq(9), Emit(TurnStarted)]` + конец пустого хода. Проверить: нет
`run.attempt_failed`; два `resync.diverged` (`expected 3, observed 2` и `expected 3, observed 9`); ход доходит до
`run.turn_ended_without_outcome`. Без правки повторный `Failure` роняет попытку.

**Документы**: contracts 4.3 (первая фраза: что ядро делает с `seq`, и про 0), 1.3 (`resync.diverged`, субъект
`instance`); `adapters.md` — события экземпляра должны приходить в порядке `seq`, один отправитель за раз;
`invariants.md` строка 20.

**Риск**: `Events::send` берёт номер и шлёт в два шага. Сегодня отправитель один (задача чтения, после неё
`child::watch`), порядок держится. Адаптер с двумя одновременными отправителями потеряет событие — громко.

---

## Записи в журнал (делает кодер по ходу)

- РЕШЕНИЕ: новые события `run.retrying`, `instance.degraded` (contracts 1.1); порог `utilization >= 1.0` в политике;
  `rate_limited` при исчерпанном лимите аккаунта паркуется как квота.
- РЕШЕНИЕ: `PromptState` сведён к `delivered` — единственное удаление, с обоснованием из п. 6.
- НАХОДКА: 6b (фильтр `daemon_crash` в `unsettled`).
- НАХОДКА: пересыльщик событий (`run/mod.rs:105`) выходит на любой ошибке `forward.send`, не только на остановке
  писателя; после этого события драйвера не доходят до конца жизни демона, попытки падают по `stall`. Вне WP-G.
- [НП]: `result.usage` у claude — за ход или накопленный.

## Вне шести пунктов (из тех же находок аудита, не трогаем)

`TurnStarted`, `State{Running | RequiresAction}` — потребитель есть: сторож тишины (contracts 4.3). `Failure.scope`,
`Source::Harness` — значения контракта журнала. `Capabilities.versions` — остаток WP-B, не событие драйвера.

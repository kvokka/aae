# WP-42 · план по аудиту А2–А5 (expert, цикл 47)

Источник: `.loop/AUDIT-2026-11-14.md` раздел А, пп. 2–5. А1 делает кодер сам, здесь его нет.
Строки — по состоянию `cbd0a6e`. Бюджет ядра 7945/12000, весь план добавляет около 110 строк продукта.
Тесты и `aae-testkit` в бюджет и в лимит 400 строк не входят (`scripts/budget.sh:11-12`).

## Разбиение и порядок

| Пакет | Что | Размер | Файлы продукта |
|---|---|---|---|
| **WP-42a** (этот цикл, вместе с А1) | А4 дренаж · А5.1 тест sweep при остановке · А5.3 `finishing` при рестарте | S+S+S | `run/gates.rs`, `run/machine.rs` (1 строка) |
| **WP-42b** | А3 инвариант 6 · А5.2 resync по маркерам · `invariants.md` | M | `process.rs`, `orphans.rs`, `run/exec.rs`, `run/registry.rs`, `run/msg.rs`, `run/mod.rs` |
| **WP-42c** | А2 пороги сторожа в конфиг | M | `config/{mod,files,check,load}.rs`, `run/{events,finish,machine,beads,queue,gates}.rs` |

Всё вместе в одну сессию (~150k) с А1 не влезает: 42b и 42c — каждая отдельная сессия. Порядок a → b → c.
Если после А1 остаётся место только на одно — брать А3 (самая серьёзная дыра).
Пересечений по строкам между пакетами нет, кроме `machine.rs` (397): 42a меняет строку 145 без прироста,
42b — строку 262 без прироста, 42c убирает 2 строки. Лимит 400 не нарушается ни в каком порядке.

---

## А4 · дренаж не запускает попытки (S) — WP-42a

**Сейчас.** `self.stopping` читает только `blocker` (`run/queue.rs:130`) — для `queued` и `parked`.
`start_attempt` (`run/machine.rs:143-147`) смотрит только ворота. Пути, которыми попытка стартует в дренаже:
`apply("preparing")` (`decide.rs:64,72`), `retry` с `after = 0` (`decide.rs:143`), дедлайн `state` у `interrupted`
(`events.rs:298-301`). Прогон в `interrupted` не входит в `active_runs` (`read.rs:89-93`), дренаж считает, что
ждать нечего, а новый процесс запускается. Аудит прав.

**Правка.** Не новый механизм, а существующий `park`: `parked` — «стартовавший прогон без процесса»,
`schedule()` в `started()` (`run/mod.rs:201`) уже поднимает припаркованных после рестарта, `blocker` уже держит
их при `stopping`.

1. `run/gates.rs` — помощник рядом с `closed_gate` (:90):
   ```rust
   /// What keeps `row` from starting an attempt: the stopping daemon (`null`, spec §5.6) or its oldest closed gate.
   pub(super) fn held(&self, tx: &Tx<'_>, row: &Row) -> Result<Option<Value>, Error> {
       if self.stopping.is_some() {
           return Ok(Some(Value::Null));
       }
       Self::closed_gate(tx, row)
   }
   ```
2. `run/machine.rs:145`: `Self::closed_gate(tx, &row)?` → `self.held(tx, &row)?` (комментарий :144 дополнить
   словами про stopping, в ту же строку). Вторую проверку ворот (:227) не трогать.
3. `run/gates.rs` `park` (:175-181): `let key = gate["key"].as_str().unwrap_or("daemon_stopping");` и `key` в
   `waiting_for` и в причине `set_state`. Слово то же, что у `queued` (`queue.rs:131`).

`active_runs` не менять: добавить туда `interrupted` — значит ждать backoff политики; после правки процесса нет.

**Тест** — `crates/aae-testkit/tests/recovery.rs`, `a_draining_daemon_parks_a_retry_and_the_next_daemon_starts_it`:
`world(POLICY)`, сценарий `Step::Exit` → `interrupted`; `Msg::Stopping { reason: "drain" }`;
`decide(Resume { after: ZERO })`. Утверждает: последнее состояние `parked`, `instance.starting` в журнале один,
`w.driver.keys().len() == 1`, `Reader::status()["active_runs"] == 0`; после `w.restart()` состояния кончаются на
`["parked", "starting", "running"]`, `instance.starting` — два.
Мутация «убрать `if self.stopping`» → прогон уходит в `starting` до рестарта → красный.

**Доки.** `contracts.md` 6.2 строка «`preparing`, `interrupted` | an attempt would start while a key … closed» —
добавить «or the daemon is stopping»; 1.3 `run.parked {gate: Gate|null}`, `run.resumed {gate_key}` =
`"daemon_stopping"` для такой парковки; `invariants.md` абзац «A planned restart drains first».

**Риски.** Инв. 14 — у `parked` дедлайн `park_timeout` есть. Инв. 16 — попытка не создаётся, потолок не тратится.
Инв. 18 — после рестарта `unpark` → `start_attempt` снова читает ворота. Замечание хука `run.finishing` в дренаже
по-прежнему шлёт промпт в ту же сессию (процесс не новый) — вне аудита, не трогать.

---

## А5.1 · sweep в пути остановки под тестом (S) — WP-42a

**Сейчас.** `exec.rs:105` (`stop_instance` → `orphans::sweep`) не ловит ни один тест testkit: существующие
(`instances.rs:215,267`) идут через `instance_failed` → op `sweep` (`exec.rs:76-79`). Аудит прав. Код не менять.

**Тест** — `crates/aae-testkit/tests/instances.rs`, `#[cfg(target_os = "linux")]`,
`a_stopped_instance_has_its_marked_descendants_swept`: `world_driven(FILES, ScriptedDriver::new(1))`, прогон до
`running`, `let orphan = marked(&w)`, `w.cancel(&run)`, `assert!(killed(orphan).await)`,
`count("instance.stopped") == 1`, `count("instance.failed") == 0`.
У сценарного драйвера `stop_instance` шлёт `Exited` при состоянии `stopping` → `instance_exited` ничего не делает
(`registry.rs:177`), единственный убийца — строка 105. Мутация «удалить строку 105» → красный.

---

## А5.3 · `finishing` при рестарте (S) — WP-42a. Формулировка аудита неверна

**Проверено прогоном** (временный тест, удалён): прогон в `finishing` переживает рестарт; `accept` → `passed` с
заявленным исходом; `remark` → `running` → промпт в мёртвый экземпляр → `harness_crash` → `interrupted` → решение
политики. Молчаливого ожидания нет: у `finishing` дедлайн `decide` лежит в состоянии (`decide.rs:34,53`).

**Почему не делать буквально** (`recover.rs:27` + `'finishing'`):
- exec-шаг в `finishing` — команда уже завершилась; `interrupted` → политика → команда запускается второй раз;
- prompt-шаг: заявленный `pass` теряется, каждый рестарт во время проверочных хуков стоит лишнего хода модели;
- в живой работе смерть процесса в `finishing` тоже не прерывает прогон (`attempt_live`, `events.rs:23`) —
  восстановление сейчас ведёт себя так же; правило записано в `contracts.md` 6.4.

**Делать:** код не менять, закрепить тестами и записью в журнал.
- `crates/aae-testkit/tests/finisher.rs` (там `hooked`, `finish`), два теста с `w.restart()`:
  `a_run_in_finishing_keeps_its_outcome_across_a_restart` — после рестарта состояние `finishing`,
  `daemon.started.interrupted_runs == 0`, `Accept` → `passed`, `run.passed.outcome.summary` прежний;
  `a_remark_after_a_restart_fails_the_attempt_instead_of_waiting` — состояния кончаются на
  `["finishing", "running", "interrupted", "failed"]`, вид сбоя `harness_crash`.
  Рецепт: `hollow_turn()` с `Step::Wait(1s)` на позиции 1, `Msg::Finish` через 300 мс, рестарт через 1.2 с.
- `invariants.md` «Recovery» п. 3: вместо «Runs in active states» — `starting`, `running`, `requires_action` →
  `interrupted`; `preparing`, `finishing`, `interrupted` сохраняют состояние (хуки отрабатывают заново).
- Журнал, РЕШЕНИЕ: «А5/`finishing`: буквальная правка повторно запускает завершённую exec-команду и теряет
  заявленный исход; оставлено правило contracts 6.4, закреплено тестами `finisher.rs`. Нюанс: после замечания
  вид сбоя — `harness_crash` от адаптера, не `daemon_crash`».

Если ревьюер потребует букву: `recover.rs:27` добавить `'finishing'` только для prompt-шагов; первый тест
перевернуть. Это 2 строки, но хуже для пользователя — только по прямому требованию.

---

## А3 · инвариант 6: ждать всё, что осталось от старого экземпляра (M) — WP-42b

**Сейчас.** Аудит прав, дыр две:
1. Путь остановки: `launch` ждёт лидера (`exec.rs:174-181`, `wait_dead`), а `sweep` идёт в эффекте
   `stop_instance` под ключом экземпляра (`exec.rs:105`) параллельно с `launch` под ключом прогона.
2. Путь краха: `instance_failed` удаляет запись и ставит `sweep` (`registry.rs:163-168`); `stopping_for`
   (`registry.rs:247-255`) берёт только записи `stopping` с handle → `after` пуст, `launch` не ждёт ничего.
   Сюда же — запись `stopping` без handle (освобождена при старте, `registry.rs:75-79`).

**Правка.** `launch` сам подтверждает смерть: лидеру даёт дожить (мягкая остановка claude: EOF → TERM → KILL,
`aae-claude/src/lib.rs:223-248`), остальное с маркером убивает и ждёт, пока `/proc` опустеет.

1. `orphans.rs:99-111` `sweep`: считать найденные, а не убитые (`.filter(kill.is_ok())` →
   `.inspect(|pid| { let _ = kill_process(*pid, Signal::KILL); })`), иначе процесс, который не удалось убить,
   сойдёт за мёртвый. Док-строка: «how many it found».
2. `process.rs` после `wait_dead` (:193):
   ```rust
   /// Waits until nothing is left of the instance `id` (invariants 6, 7): its process `leader` (pid, start time)
   /// gets `limit` to end by itself, then every process still carrying the marker is killed. `false` past `limit`.
   pub(crate) async fn wait_gone(id: &str, leader: Option<(i32, u64)>, limit: Duration) -> bool {
       let deadline = tokio::time::Instant::now() + limit;
       if let Some((pid, start_time)) = leader
           && !wait_dead(pid, start_time, limit).await
       {
           return false;
       }
       loop {
           let marker = id.to_owned();
           // Walking `/proc` is synchronous.
           let found = tokio::task::spawn_blocking(move || crate::orphans::sweep(&marker)).await;
           if found.unwrap_or(0) == 0 {
               return true;
           }
           if tokio::time::Instant::now() >= deadline {
               return false;
           }
           tokio::time::sleep(Duration::from_millis(50)).await;
       }
   }
   ```
3. `run/registry.rs:245-255`: `stopping_for` → `old_instances`, отдаёт `[{ "id", "handle" | null }]`:
   ```sql
   SELECT DISTINCT a.instance, i.handle FROM attempts a LEFT JOIN instances i ON i.id = a.instance
   WHERE a.run IN (?1, ?2) AND a.instance IS NOT NULL AND (i.id IS NULL OR i.state = 'stopping')
   ```
   Условие по состоянию обязательно: живой общий экземпляр (`starting`/`ready`, codex app-server) в список не
   попадает никогда. Вызов — `machine.rs:262`, стоит после `UPDATE attempts SET instance` (:257): экземпляр
   новой попытки `starting`/`ready` и сам себя не ждёт.
4. `run/exec.rs:174-181`:
   ```rust
   for old in p["after"].as_array().into_iter().flatten() {
       let leader = serde_json::from_value::<InstanceHandle>(old["handle"].clone()).ok();
       let id = old["id"].as_str().unwrap_or_default();
       if !crate::process::wait_gone(id, leader.map(|h| (h.pid, h.start_time)), DEATH_WAIT).await {
           return Err(Failure::new(FailureKind::Unknown, Source::Core));
       }
   }
   ```
   Строки 76-79 и 105 не трогать. Совместимость формы `after` не нужна: `launch` активных прогонов
   восстановление удаляет (`recover.rs:33-36`).

**Тесты** (мутации две — убрать вызов и убрать половину ожидания — ловятся разными тестами):
- `crates/aae-testkit/tests/instances.rs`, Linux, `[FILES, POLICY].concat()`, `ScriptedDriver::new(1)`:
  `a_new_attempt_starts_only_after_every_process_of_the_old_instance_is_gone`. `Step::Exit` → `instance.failed`;
  дождаться, пока в `state.db` не останется эффекта `op = 'sweep'` (иначе сирота умрёт от штатного sweep и
  мутация выживет); `let mut orphan = marked(&w)`; `decide(NewSession)`; дождаться второго `running`;
  `assert!(orphan.try_wait().unwrap().is_some())` — без ожидания; `count("instance.ready") == 2`.
  Мутация «убрать цикл `after`» или «вернуть старый SQL» → сирота жива → красный.
- Там же `a_new_attempt_on_a_shared_instance_leaves_its_processes_alone`: `ScriptedDriver::new(2)`, два прогона
  на одном экземпляре, `marked(&w)`, `w.crash(<сессия первого>)`, `decide(NewSession)`, второй `running` →
  `try_wait().is_none()`. Мутация «убрать условие по состоянию в SQL» → красный. Это страховка от самой опасной
  регрессии — убить живой app-server.
- `process.rs` tests (рядом с :289), `wait_gone_spares_a_live_leader_and_kills_what_is_left_after_it`:
  `sh -c 'setsid sleep 300 & exec sleep 300'` с маркером через `orphans::spawn`, `process_group(0)`;
  `!wait_gone(&id, Some((pid, start)), 200ms)` и лидер жив (`start_time(pid) == Some(start)`); убить лидера,
  `wait`, `waited`; `wait_gone(.., 5s)` и `orphans::sweep(&id) == 0`.
  Мутация «убрать ожидание лидера» → первый вызов убивает лидера и возвращает `true` → красный.
  Мутация «убрать цикл sweep» → потомок жив → красный.

**Доки.** `contracts.md` 8, пункт «Confirmed death (invariant 6)» (~:615) — новое правило.
`invariants.md`: строка 6; абзац «How they are checked» (:37-42) — убрать 6 из списка chaos, назвать тесты
выше; `chaos/check.rs:124` — метка «(invariant 2)»: проверка «≤ 1 открытой попытки» — это владение.
Сжатый §5.5 спеки (аудит Б20) правит ревьюер — запись в журнал, спеку не трогать.

**Риски.** Инв. 9 — `launch` убивает только маркеры экземпляров, чья остановка уже записана (`stopping` или
запись удалена), нового намерения нет. Инв. 7 — усиливается. Хоп (критерий 6) — у первой попытки `after` пуст,
цена ноль; у N-й до N−1 обходов `/proc` по несколько мс. Paused-тесты: tokio 1.54.1 не двигает часы, пока идёт
`spawn_blocking` (`runtime/blocking/schedule.rs:25`), проверено. Не закрыто: старый экземпляр `ready` с другим
fingerprint живёт своё `instance_idle` при закрытой сессии — так устроен общий реестр (§7.3), в `invariants.md`
одной фразой. macOS: обход `/proc` пуст, остаётся ожидание лидера, как сейчас.

---

## А5.2 · resync сверяет процессы с маркерами (M−) — WP-42b

**Сейчас.** Resync проверяет только «запись есть → процесс жив» (`registry.rs:183-205`, `exec.rs:64-75`).
Обратной сверки нет; при старте `check_instances` не вызывается вовсе (`run/mod.rs:195-202`). Аудит прав.

**Ограничение, которое определяет дизайн.** На хосте бывает несколько демонов (тесты, acceptance, два
оркестратора). «Убить любой неизвестный маркер» убьёт чужие экземпляры. Свой маркер отличим только по истории:
`attempts.instance` (индекс `attempts_instance` есть, `store.rs:72`). Исполнитель эффекта в БД не ходит —
решает писатель.

**Правка.**
1. `orphans.rs`: вынести чтение окружения в `fn marker(pid: i32) -> Option<String>` (значение
   `AAE_INSTANCE`), `sweep` и тестовый `marked` — через него; добавить
   `pub(crate) fn markers() -> BTreeSet<String>` (все маркеры живых процессов, кроме себя и зомби).
2. `run/msg.rs`: `Marked { ids: Vec<String> }` — «the instance markers the resync found in the process table».
3. `run/exec.rs`, ветка `check_instances` (после цикла health, :75): `spawn_blocking(orphans::markers)` и
   `core.send(None, Msg::Marked { ids })` — всегда, и с пустым списком (число сообщений не зависит от хоста).
4. `run/registry.rs`: в `check_instances` убрать ранний выход `if instances.is_empty()` (:197-199); новая
   ```rust
   /// The resync found processes carrying these markers. One of an instance this daemon ran and no longer records
   /// is a sweep that was outrun (a descendant forked under it): it is swept again (invariants 7, 20). A recorded
   /// instance is its own process, and a marker this state never gave belongs to another daemon: both are left.
   pub(super) fn marked(tx: &mut Tx<'_>, ids: &[String]) -> Result<(), Error>
   ```
   на каждый id: `SELECT EXISTS (SELECT 1 FROM attempts WHERE instance = ?1) AND NOT EXISTS (SELECT 1 FROM
   instances WHERE id = ?1)` → `tx.effect(id, "sweep", &json!({ "op": "sweep", "instance": id }))`.
5. `run/mod.rs`: ветка `Msg::Marked`; в `started()` после `self.recover(tx)?` — `Self::check_instances(tx)?`
   (resync при старте, §5.6 п. 4).

События нет: потерянный sweep однозначен и доводится, как `instance_gone` (`registry.rs:207-209`);
`resync.diverged` по контракту — «без эффекта», здесь эффект есть. Записать в журнал как РЕШЕНИЕ.

**Тест** — `instances.rs`, Linux, `the_resync_sweeps_what_carries_the_marker_of_a_gone_instance`, две фазы:
прогон, `cancel`, дождаться `instance.stopped` (sweep остановки прошёл); `late = marked(&w)`;
`foreign = sleep 300` с `AAE_INSTANCE=foreign-<pid теста>`; `sleep(300 s)` → `killed(late)`,
`foreign.try_wait().is_none()`. Вторая фаза: ещё один `marked(&w)`, `w.restart()` → `killed`.
Мутации: убрать отправку/обработку `Marked` → `late` жив; «мести любой незаписанный маркер» → `foreign` убит;
убрать `check_instances` из `started()` → вторая фаза красная.

**Доки.** `event-model.md` «Resync» (:43-56), `contracts.md` 8 пункт «Resync» (~:624), `invariants.md` строка 20
и «Recovery» п. 4.

**Риски.** Маркеры сервисов и exec-шагов без записи не сверяются (истории их id в состоянии нет) — ограничение
одной фразой в `event-model.md`, код «на вырост» не писать. Цена — один обход `/proc` раз в 5 минут и при
старте. Chaos: добавляется эффект и сообщение на каждый resync даже без экземпляров — последовательности
изменятся, сиды случайные (§2.1 п. 20), прогнать chaos 3 раза.

---

## А2 · пороги сторожа в конфиг (M) — WP-42c

**Сейчас.** Семь констант: `events.rs:17-20` (`SILENCE_MS`, `TOOL_SILENCE_MS`, `BACKGROUND_SILENCE_MS`,
`HUMAN_TIMEOUT_MS`), `machine.rs:18-19` (`QUEUE_TIMEOUT_MS`, `START_TIMEOUT_MS`), `gates.rs:13`
(`PARK_TIMEOUT_MS`). `contracts.md:14` называет каждое «default» параметром конфига. Аудит прав.
Вне объёма: `STEP_TIMEOUT_MS` (`machine.rs:20`) — умолчание ключа `timeout` формулы, уже настраивается;
`RESYNC_MS`, `DEATH_WAIT`, `STOP_GRACE`, `HEALTH_EVERY`, `DECIDE_SLACK_MS` — не пороги инварианта 15.

**Конфиг** — таблица в файле оркестратора, уровень как у `max_runs` (в слои настроек не идёт: §6.2 их не
называет). Длительности строками с единицей (`contracts.md:11`):
```toml
[watchdog]                  # every key optional; a reload applies to deadlines set from then on
silence = "5m"              # no event after the ack, or the model thinking
tool_silence = "10m"
background_silence = "30m"
human_timeout = "1h"
start_timeout = "120s"
queue_timeout = "168h"
park_timeout = "168h"
```

**Правка.**
1. `config/mod.rs`: `pub struct Watchdog` (`Clone, Copy, Debug, PartialEq`), семь полей `i64` в мс
   (`silence_ms` … `park_timeout_ms`); поле `pub watchdog: Watchdog` в `Config`. Миллисекунды — потому что все
   места применения складывают с `tx.now_ms()`.
2. `config/files.rs` `OrchestratorFile` (:98): `#[serde(default)] pub watchdog: BTreeMap<String, String>`.
3. `config/check.rs` (198 строк, место есть; в `load.rs` 385 — нет): `pub(super) fn watchdog(loader, file, raw)
   -> Watchdog` — структура с умолчаниями литералом, цикл по ключам, один `match` «ключ → `&mut` поле»,
   неизвестный ключ — ошибка, значение через `super::duration`, допустимо от 1 мс до 8760 ч (верхняя граница
   снимает переполнение `now + ms` и `rfc3339_ms`, `saturating_add` в местах применения не нужен). ~24 строки.
4. `config/load.rs`: `let watchdog = super::check::watchdog(&mut self, file, raw.watchdog);` рядом с `bundled`
   (:44) и поле в литерале (:68) — +2 строки, станет 387.
5. Места применения (полный список по grep), константы удалить:
   - `events.rs`: :135, :150, :193, :283 → `silence_ms`; :143 → `tool_silence_ms`; :155 →
     `background_silence_ms`; :179 → `human_timeout_ms`. В `attempt_event` — `let w = self.config.watchdog;`.
   - `finish.rs:6` (импорт), :115 → `silence_ms`.
   - `machine.rs:279` и `beads.rs:92` → `start_timeout_ms`; `claim` становится `&self`, вызов `machine.rs:37`.
   - `queue.rs:4` (импорт), :110 → `queue_timeout_ms`.
   - `gates.rs:184` → `park_timeout_ms`.

**Тесты.**
- `crates/aae-core/src/tests/config.rs` (или `config_layers.rs`, там свободнее):
  `the_watchdog_deadlines_come_from_the_orchestrator_file`: без таблицы — умолчания
  `{300_000, 600_000, 1_800_000, 3_600_000, 120_000, 604_800_000, 604_800_000}`; все семь ключей с разными
  значениями — точное равенство структуре; `fails`: неизвестный ключ, `"0s"`, `"5 minutes"`.
  Ловит: перепутанные ключи в `match`, опечатку в умолчании, снятую проверку нуля.
- `crates/aae-testkit/tests/watchdog.rs`, `every_deadline_comes_from_the_config`: конфиг с семью разными
  короткими значениями, по миру на сценарий, точное `ts` события (часы управляемые, с нуля):

  | Ключ = значение | Сценарий | Ожидание |
  |---|---|---|
  | `start_timeout = "30s"` | `Step::NoAck` | `run.attempt_failed` `stall` в `00:00:30.000` |
  | `silence = "1m"` | без сценария | `stall` в `00:01:00.000` |
  | `tool_silence = "2m"` | `TurnStarted`, `Activity { tool_running: true }` | `stall` в `00:02:00.000` |
  | `background_silence = "3m"` | `hollow_turn()` с `background: true` | `stall` в `00:03:00.000` |
  | `human_timeout = "4m"` | `PermissionRequested` (как `requests.rs:43`) | возврат в `running` в `00:04:00.000` |
  | `queue_timeout = "6m"` | `CloseGate` на риг, затем `create` | `run.failed` `queue_timeout` в `00:06:00.000` |
  | `park_timeout = "7m"` | как `gates.rs:122-146` | `run.failed` `park_timeout` через 7 мин после парковки |

  Ловит любое место применения, читающее не то поле или литерал. Существующие тесты на умолчаниях
  (`runs.rs:70`, `requests.rs:43`, `gates.rs:141`, `beads.rs:321`) остаются зелёными без правок.

**Доки.** `config.md` блок оркестратора (:121-147) — таблица `[watchdog]` и фраза про reload; `contracts.md` 6.1
под таблицей состояний — где задаются; `invariants.md` строка 15 — ссылка на `config.md`.
Журнал, РЕШЕНИЕ: имя таблицы и ключей, уровень оркестратора, границы 1 мс – 8760 ч.

**Риски.** Инв. 14 — ноль и переполнение отсечены загрузчиком. Reload: уже взведённые дедлайны остаются со
старым значением. `scripts/unused-pub.sh` — `Watchdog` используется в `check.rs`, проходит.

---

## Проверка перед коммитом каждого пакета

```bash
cargo clippy --all-targets -q --message-format short -- -D warnings 2>&1 | head -60
cargo nextest run -p aae-core -p aae-testkit --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -80
mise exec -- bash scripts/budget.sh 2>&1 | tail -12
```
Мутации из разделов «Тест» прогнать руками по одной (правка → нужный тест красный → откат) и перечислить в
отчёте цикла: ревьюер проверяет пункты аудита именно так. Для 42b — chaos три раза подряд.

# WP-135 · `max_writers` — план для исполнителя

Цель: ключ `max_writers` в `[rigs.<name>]`; ядро держит в `queued` прогон, который стал бы лишним в собственном
каталоге рига. Схема БД не меняется (версия 24 остаётся), новых состояний и парковок нет. Ожидаемый объём: ~75 строк
продукта, ~280 строк тестов, ~45 строк документов; 18 файлов.

## 1. Семантика

**Ключ.** `max_writers` — положительное целое в `[rigs.<name>]`, без ключа лимита нет. `0` — ошибка конфига (serde,
`NonZeroU32`). `[root]` ключ не принимает (`deny_unknown_fields` уже даёт ошибку): у root остаётся открытым вопрос о
сессиях агентов scope `root`, которые работают в его каталоге из прогонов любого рига (вопрос владельцу — в журнале).

**Кто считается (writer).** Свойство прогона статично, считается по формуле и конфигу, а не по текущему шагу — иначе
прогон «занимал бы место» посреди работы без проверки. Прогон формулы `f` на риге `r` считается, когда:
- у формулы нет `worktree = true`, **и**
- хотя бы один шаг работает в каталоге рига: шаг `exec`, либо шаг `ai`, чей агент на этом риге
  (`Config::for_run_as(project, rig, formula, step, as_agent)`) не scope `root`. Шаги `drain`, `formula`, `await`
  процесса не имеют. Формула только из них или только из `ai`-шагов root-агентов — не writer.
- Формула исчезла после reload или агент не резолвится — считается (осторожная сторона).

Поля: `runs.project`, `runs.rig`, `runs.formula`, `runs.as_agent`, `runs.state`, `runs.step_kind`, `runs.parent_run`
— все уже есть. `runs.workdir` не используется (до `prepared` там путь рига и у worktree-прогона).

**Какие состояния занимают место.** Все, кроме `queued`, `passed`, `failed`, `cancelled`. `parked` и `interrupted`
занимают (отличие от `started()`: там `parked` и шаги без процесса слот отпускают). Место освобождается только
терминальным переходом.

**Кого проверяют.** Только прогон в `queued`, и только если он сам writer. Запаркованный прогон место уже держит:
его `unpark` лимитом не проверяется (иначе после снижения лимита два запаркованных заблокируют друг друга, а при
лимите 1 запаркованный заблокирует сам себя — в запросе нет `id != self`, он не нужен: кандидат всегда `queued`).

**`waiting_for`.** Новое значение `"writers"` (не `concurrency`: причины разные, чинятся разными ключами; contracts
1.3 требует видимой причины). `gate` в событии — `null`. Порядок причин в `blocker`: `daemon_stopping`, `gate`,
`concurrency`, `writers`. `run.resumed.gate_key` не меняется (по `writers` не паркуют). OpenAPI: меняется только
описание поля `waiting_for` (строка, не enum) — из rustdoc `replies.rs`, файл перегенерируется.

## 2. Дочерние прогоны `drain` / `formula`

Правило: **прогон в шаге `drain` или `formula` место держит, но одалживает его прогонам, которых этот шаг запустил, и
их потомкам по такой же цепочке** — против них он не считается. Против чужих прогонов считается как обычно.

Почему так, а не «прогон в шаге без процесса не считается вовсе»: тогда чужой прогон занял бы каталог, пока родитель
в `drain`, и родитель вышел бы в следующий шаг вторым писателем без проверки (понадобилась бы парковка на границе
шага, а запаркованные по условию считаются — круг). `await` не одалживает: дерево полусделано, ждёт человека.

Почему нет взаимоблокировки: у родителя в `drain`/`formula` нет процесса, для своих детей он невидим, дети
конкурируют только между собой и с чужими; при лимите 1 дети идут по одному (`pool` сверху). Шаг родителя кончается,
когда детей шага не осталось в живых, — к его следующему шагу место снова только его. Цепочка рекурсивна: внук
(`drain` → формула с шагом `formula` → прогон) пропускает и родителя, и деда, пока каждое звено стоит в
`drain`/`formula`.

Прогон, запущенный из сессии (`run start` с токеном), — не такой ребёнок: его родитель в `ai`-шаге работает в
каталоге и считается. При лимите 1 он ждёт конца родителя (`waiting_for = "writers"`); сессия, которая ждёт такого
ребёнка через `run wait`, прождёт до таймаута шага — это и есть «один писатель», выход — `worktree = true` у формулы
ребёнка или лимит выше. В документе сказать одной фразой.

Запрос (проверен на SQLite; форма через JOIN не имеет ловушки `NOT IN (NULL)`):

```sql
WITH RECURSIVE lender(id, parent) AS (
    SELECT p.id, p.parent_run FROM runs c JOIN runs p ON p.id = c.parent_run
    WHERE c.id = ?3 AND p.step_kind IN ('drain', 'formula')
    UNION ALL
    SELECT p.id, p.parent_run FROM lender JOIN runs p ON p.id = lender.parent
    WHERE p.step_kind IN ('drain', 'formula')
)
SELECT formula, as_agent FROM runs
WHERE project = ?1 AND rig = ?2 AND state NOT IN ('queued', 'passed', 'failed', 'cancelled')
  AND id NOT IN (SELECT id FROM lender)
```

Строки фильтруются в Rust через `writes(...)`, заполнен ли риг: `count >= limit`.

## 3. Где проверять и как будить

- Новый файл `crates/loop-manager-core/src/run/writers.rs` (`queue.rs` — 377 строк из 400, туда только вызов):
  - `fn writes(&self, project: &Project, rig: &str, formula: &str, as_agent: Option<&str>) -> bool` — правило п. 1;
  - `pub(super) fn rig_full(&self, tx: &Tx<'_>, row: &Row) -> Result<bool, Error>` — `false`, если у
    `project.rigs.get(&row.rig)` нет `max_writers` (ранний выход: без ключа ноль работы) или сам `row` не writer;
    иначе запрос п. 2 и подсчёт.
- `queue.rs::blocker`: после ветки `concurrency` —
  `let full = row.state == "queued" && self.rig_full(tx, row)?; Ok(full.then_some(("writers", Value::Null)))`.
  Поправить rustdoc `blocker` и шапку модуля. `agent_full`, `started()`, `held()` (gates.rs), `park`/`unpark` не
  трогать.
- Пробуждение: кода не нужно. Место освобождает только терминальный переход — `machine.rs::terminal` уже зовёт
  `schedule`; вход родителя в `drain` — `start_drain` зовёт `schedule`; в `formula` — ребёнок создаётся через
  `create`, там `blocker` + `schedule`.
- Рестарт: кода не нужно. Счёт выводится из таблицы `runs` и конфига; `started()` в `run/mod.rs` после `recover`
  зовёт `schedule`. `interrupted` после рестарта место держит.
- Reload: в `reload.rs::reload` после `tx.emit(config.reloaded)` добавить `self.schedule(tx)?;` — иначе прогон,
  ждущий снятого/поднятого лимита, стоит до чужого события. Снижение лимита никого не останавливает и не паркует:
  новые не стартуют, пока живых не станет меньше лимита. Если строка ломает больше одного существующего теста ядра —
  не чинить тесты вслепую, вернуть expert'у с именами тестов.

## 4. Файлы

Код (9):
1. `crates/loop-manager-contract/src/config/files.rs` — `RigFile`: `#[serde(default)] pub max_writers: Option<NonZeroU32>`.
2. `crates/loop-manager-contract/src/config/mod.rs` — `Rig`: `pub max_writers: Option<u32>` с rustdoc (одна строка).
3. `crates/loop-manager-contract/src/config/places.rs` — `place_of`: `max_writers: None`; в цикле ригов
   `place.max_writers = r.max_writers.map(NonZeroU32::get)`.
4. `crates/loop-manager-core/src/run/writers.rs` — новый, ~50 строк (п. 3).
5. `crates/loop-manager-core/src/run/mod.rs` — `mod writers;`.
6. `crates/loop-manager-core/src/run/queue.rs` — вызов в `blocker`, rustdoc.
7. `crates/loop-manager-core/src/run/reload.rs` — `self.schedule(tx)?`.
8. `crates/loop-manager-front/src/api/config.rs` — в `explained`, ветка `Explain::Formula`, сразу после строки
   `rounds`: если `p.rigs.get(rig)` задаёт ключ — `max_writers = {n}  # rig\n`. Ветку `--agent` не трогать.
9. `crates/loop-manager-front/src/api/replies.rs` — rustdoc `waiting_for`: добавить `writers`.

Тесты (4): `crates/loop-manager-testkit/tests/core/writers.rs` (новый) + строка `mod writers;` в `main.rs`;
`crates/loop-manager-contract/src/tests/config_root.rs`; `crates/loop-manager-front/src/api/tests.rs`.

Документы (5), всё по-английски, без истории:
- `docs/design/config.md`:
  - «Project», пример: под `[rigs.backend]` строка
    `max_writers = 1  # optional positive integer: runs that work in the rig's directory at once (default: no limit)`;
  - там же новый абзац после абзаца про prefix: что считается (п. 1), `parked` считается и почему (в отличие от
    `max_runs` агента), `waiting_for = "writers"`, правило одалживания (п. 2), фраза про прогон из сессии, «a reload
    that changes the limit ends no run; the queue takes the new limit at once»;
  - абзац про root: «`[root]` takes no `settings`, `agents`, `triggers` or `max_writers`»;
  - «Formula», конец абзаца про `worktree`: «loop-manager does not lock a directory: without a worktree, runs of one
    rig share its directory, and keeping them apart is the user's choice (`worktree`, the rig's `max_writers`, `pool`,
    `max_runs`)»;
  - «Effective config and explain»: после `rounds` — «then `max_writers` of the rig when it sets one
    (`max_writers = 1  # rig`)»;
  - «Loading», пункт про reload: очередь проходится заново после reload.
- `docs/design/contracts.md`: 1.3 `run.queued` — значение `"writers"`; 6.1 строка `queued` и пункт «`queued` has not
  started» — «a concurrency slot, a place in its rig (`max_writers`) or an open gate»; пункт про workdir (строки
  ~703–705) — убрать «does not serialize runs by directory», сказать: каталог не блокируется, по умолчанию два прогона
  работают в одном каталоге, `max_writers` рига ставит лишних в очередь; новый пункт рядом с «Concurrency is bounded»
  — кто считается, состояния, одалживание; 6.2 `queued → preparing` — «slot free, a place in the rig free»; после
  таблицы — «Every terminal transition frees the run's concurrency slot and its place in its rig».
- `docs/agents/decisions.md`, 108: «The core does not hold a workdir: a run is bound to a directory and loop-manager
  does not lock it. Several runs may work in one rig's directory at once; keeping them apart is the user's choice:
  `worktree = true`, or the rig's `max_writers`, by which the core queues the runs that work in the rig's own
  directory (no limit by default) (2026-11-17, revised 2026-11-19)».
- `docs/agents/assumptions.md`, «Runs and triggers»: пункты-допущения исполнителя — writer определяется формулой
  целиком; шаг `drain`/`formula` одалживает место потомкам; прогон из сессии считает родителя; причина `writers`;
  `[root]` ключа не имеет.
- `docs/api/openapi.json` — только перегенерация:
  `LOOP_MANAGER_UPDATE_OPENAPI=1 cargo nextest run -p loop-manager-front openapi`.

`docs/design/invariants.md` не меняется (инвариантов лимит не добавляет и не трогает).

## 5. Тесты

Стенд — core-тесты testkit без демона, управляемые часы (`#[tokio::test(start_paused = true)]`), образцы:
`runs.rs::the_orchestrator_and_the_agent_limit_started_runs`, `gates.rs`, `drain_pool.rs`, `formula_step.rs`,
`root.rs` (scope root), `returns.rs:222-225` (reload). Общий мир: `shop/loop-manager.toml` =
`[rigs.web] path = "web", max_writers = 1` + `[rigs.api] path = "api"`, `shop/api/.keep`.

`tests/core/writers.rs`:
1. `a_rig_at_its_max_writers_queues_the_next_run_until_one_ends` — прогон на `api`, затем A на `web` доходит до
   `running`; B на `web`: последний `run.queued.waiting_for == "writers"`, `states(B)` пуст; ещё один на `api`
   стартует; cancel A → `states(B) == ["preparing", "starting", "running"]`.
   Убивает: `>=` → `>`; потерю фильтра `rig = ?2`; отсутствие проверки вообще.
2. `a_parked_run_keeps_its_place` — мир `POLICY`, A с `crash()`, решение `Decision::Park` на ключ, который есть
   только у A (`agent:<run.agent из RunView>`, B — формула другого агента; либо `bead:shop/<id>` через
   `start_bead`); B → `writers`; `OpenGate` → у A `run.resumed`, B всё ещё `queued`; cancel A → B стартует.
   Убивает: исключение `parked` из счёта (копия `started()`); проверку лимита у запаркованного (A не проснулся бы).
3. `a_run_in_a_worktree_of_its_own_takes_no_place` — формула `tree` с `worktree = true`. W (tree) стоит в
   `preparing` (хуки мира `Lost` не отвечают; проверить состояние явно) → A (build) доходит до `running`; затем при
   живом A второй W выходит из `queued`. Убивает: потерю `!f.worktree` в обе стороны.
4. `a_run_that_only_works_in_the_root_takes_no_place` — агент `lead` со `scope = "root"`; формула `plan` (один
   `ai`-шаг lead): её прогон на `web` не мешает A стартовать. Формула `mixed` (`ai` lead, затем `ai` dev): её
   прогон на первом шаге уже держит место — следующий `build` ждёт `writers`.
   Убивает: потерю правила scope; подмену «любой шаг» на «текущий шаг».
5. `a_drain_under_max_writers_one_gives_its_beads_one_at_a_time` — файлы `common::drain::SWEEP`, но `config.toml`
   без `max_runs = 1` (иначе лимитом будет он) и `loop-manager.toml` с `max_writers = 1`; корневой bead + 3 под ним
   (`add_in("w", …)`). Пока идёт `drain`: ровно один ребёнок `running`, остальные `queued` с `writers`; посторонний
   `build` на `web` — `queued` с `writers`; `settle` → `3 closed, 0 failed, 0 refused`; `wrap` → `run.passed`;
   посторонний стартует. Убивает: потерю одалживания (шаг не сойдётся); «прогон в `drain` не считается ни для кого».
6. `a_formula_step_inside_a_formula_step_is_not_blocked` — `outer` (`formula` → `mid`, затем `ai` dev), `mid`
   internal (`formula` → `leaf`), `leaf` internal (`ai` dev); прогон `leaf` доходит до `running`.
   Убивает: нерекурсивную цепочку (только прямой родитель).
7. `a_reload_that_lifts_the_limit_starts_the_waiting_run` — A идёт, B ждёт `writers`; переписать
   `loop-manager.toml` без ключа, `Msg::Reload` → B выходит из `queued`. Убивает: отсутствие `schedule` в reload.

Файл не длиннее 400 строк; не влезает — тесты 5–6 в `writers_drain.rs`.

`config_root.rs` (contract): `a_rig_takes_max_writers_and_the_root_does_not` — значение читается в
`Rig::max_writers`; `max_writers = 0` — ошибка; `[root] max_writers = 1` — ошибка.

`api/tests.rs` (front): `a_formula_on_a_rig_is_explained_with_its_max_writers` — с `rig = Some("web")` текст
начинается с `rounds = …\nmax_writers = 1  # rig\n`; без рига (root) строки нет.

Мутационная проверка (условие «готово»): руками, по одной, с откатом — (а) убрать `!f.worktree`, (б) добавить
`'parked'` в список исключённых состояний, (в) убрать `AND id NOT IN (SELECT id FROM lender)`, (г) `>=` → `>`,
(д) убрать `row.state == "queued" &&`. После каждой — `cargo nextest run -p loop-manager-testkit --test core writers`;
в отчёт — какая мутация каким тестом убита. Выжившая мутация — недостающий тест, а не повод её пропустить.

Прогоны по ходу: `cargo nextest run -p loop-manager-contract config_root`, `-p loop-manager-front`,
`-p loop-manager-testkit --test core writers`; один раз весь `--test core` (из-за `schedule` в reload и правки
`blocker`). `mise run ci` — у coder.

## 6. Риски и чего не делать

- Не менять схему БД, `Row`, `started()`, `agent_full`, `held`, `park`/`unpark`; не парковать по `writers`.
- Не добавлять: `max_writers` в `[root]`, в `GET /v1/projects`, в `explain --agent`; определение каталога по
  `runs.workdir`; acceptance-сценарий, демо, правки `demos/` и guides.
- Известная осторожная неточность (в документ не выносить, в отчёт — одной строкой): прогон с `continue_from`
  от worktree-прогона работает в старом worktree, но по формуле без `worktree` считается writer'ом.
- Снижение лимита reload'ом ниже числа живых: две семьи `drain` на одном риге могут ждать друг друга до конца одной
  из них (причина видна, лечится лимитом или cancel). Не решать кодом.
- `for_run_as` собирает эффективный конфиг на каждый `ai`-шаг каждого живого прогона рига при каждом проходе
  очереди — только для ригов с ключом; прогонов единицы. Не кэшировать.
- `machine.rs` и `hooks.rs` стоят на 400 строках — туда ничего.
- Тексты ошибок, rustdoc, документы — английский; `.loop/` — русский.

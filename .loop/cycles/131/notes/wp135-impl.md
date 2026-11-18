# WP-135 · `max_writers` — отчёт исполнителя

## Что сделано

Ключ `max_writers` в `[rigs.<name>]` (NonZeroU32, `0` — ошибка serde «expected a nonzero u32», в `[root]` — «unknown
field»). Ядро держит в `queued` с `waiting_for = "writers"` прогон-писатель, когда у рига уже `max_writers` живых
писателей. Правило подсчёта, одалживание мест `drain`/`formula`-шагов, `schedule` после reload — по плану expert.
Схема БД, `Row`, `started()`, `agent_full`, `held`, `park`/`unpark` не тронуты.

## Файлы

Код:
- `crates/loop-manager-contract/src/config/files.rs` — `RigFile::max_writers: Option<NonZeroU32>`;
- `crates/loop-manager-contract/src/config/mod.rs` — `Rig::max_writers: Option<u32>`;
- `crates/loop-manager-contract/src/config/places.rs` — `place_of` (`None`) и перенос значения в цикле ригов;
- `crates/loop-manager-core/src/run/writers.rs` — новый (65 строк): `writes`, `rig_full`;
- `crates/loop-manager-core/src/run/mod.rs` — `mod writers;`;
- `crates/loop-manager-core/src/run/queue.rs` — ветка `writers` в `blocker`, rustdoc и шапка модуля (381 строка);
- `crates/loop-manager-core/src/run/reload.rs` — `self.schedule(tx)?` после `config.reloaded`;
- `crates/loop-manager-front/src/api/config.rs` — строка `max_writers = N  # rig` в `explain --formula`;
- `crates/loop-manager-front/src/api/replies.rs` — rustdoc `waiting_for`.

Тесты: `crates/loop-manager-testkit/tests/core/writers.rs` (новый, 233 строки, 7 тестов) + `main.rs`;
`crates/loop-manager-contract/src/tests/config_root.rs` (`a_rig_takes_max_writers_and_the_root_does_not`);
`crates/loop-manager-front/src/api/tests.rs` (`a_formula_on_a_rig_is_explained_with_its_max_writers`).

Документы: `docs/design/config.md` (пример `[rigs.backend]`, абзац о `max_writers`, `[root]`, абзац `worktree` в
«Formula», explain, «Loading» о reload), `docs/design/contracts.md` (1.3 `run.queued`, 6.1 строка `queued`, пункт
`queued`, пункт про workdir, новый пункт о писателях, 6.2 `queued → preparing`, фраза после таблицы),
`docs/agents/decisions.md` (108 — текст плана), `docs/agents/assumptions.md` («Runs and triggers», один пункт),
`docs/api/openapi.json` (перегенерирован, меняется только описание `waiting_for`).

## Отступления от плана

- `#[serde(default)]` у `RigFile::max_writers` не ставил: `Option` и так по умолчанию `None` (как `rounds` рядом).
- `blocker`: ветка `concurrency` переписана из `let full = …; Ok(full.then_some(…))` в ранний `return`, затем ветка
  `writers` — по смыслу то же, что в плане.
- Rustdoc `waiting_for` укорочен до одной строки («`writers` (its rig's `max_writers`)»), чтобы в openapi не было `\n`.
- Тест 3 (worktree): в мире с `worktree = true` хук `worktree` (Lost) висит у **всех** прогонов проекта, не только у
  worktree-формулы, поэтому `build` тоже стоит в `preparing`, а не доходит до `running`. Проверка — «вышел из
  `queued`» (`states == ["preparing"]`); мутация (а) этим всё равно убивается.
- Тест 6: `outer` в шаге `formula` проходит `preparing → running` без `starting`; лист ищется по `run.queued` с
  формулой `…leaf` (`World::kids` падает на `bead = NULL`).
- Хелпер отмены назван `cancel_run` (в `instances.rs` уже есть приватный `World::cancel`, конфликт inherent impl).

## Проверки

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` — чисто.
- `cargo nextest run -p loop-manager-testkit -p loop-manager-contract -p loop-manager-front -p loop-manager-core
  -E 'not binary(chaos)'` — 481/481 (весь `--test core` включён; `schedule` в reload не сломал ни одного теста).
- openapi-тест зелёный после перегенерации.
- `LOOP_MANAGER_CHAOS_SEEDS=96 … --test chaos` — 10/10.
- e2e `guides::` и `explain` — 3/3.
- `scripts/budget.sh` — exit 0 (core 11988/18000, total 18905/30000); `core-boundary`, `select-biased`,
  `unused-pub` — exit 0.
- `mise run ci` не гонял (по поручению — у coder).

## Мутации (каждая откатана, файлы сверены с копией)

| Мутация | Чем убита |
|---|---|
| (а) убрать `!f.worktree &&` | `a_run_in_a_worktree_of_its_own_takes_no_place` |
| (б) `'parked'` в список исключённых состояний | `a_parked_run_keeps_its_place` |
| (в) убрать `AND id NOT IN (SELECT id FROM lender)` | `a_drain_under_max_writers_one_gives_its_beads_one_at_a_time`, `a_formula_step_inside_a_formula_step_is_not_blocked` |
| (г) `>=` → `>` | 5 тестов (лимит 1, parked, root, drain, reload) |
| (д) убрать `row.state == "queued" &&` | `a_parked_run_keeps_its_place` (A не просыпается) |
| доп.: убрать фильтр `rig = ?2` | `a_rig_at_its_max_writers_queues_the_next_run_until_one_ends` |
| доп.: нерекурсивная цепочка (рекурсивная часть `WHERE 0`) | `a_formula_step_inside_a_formula_step_is_not_blocked` |
| доп.: правило scope всегда «пишет» | `a_run_that_only_works_in_the_root_takes_no_place` |
| доп.: убрать `schedule` в reload | `a_reload_that_lifts_the_limit_starts_the_waiting_run` |

Выживших нет.

## `config explain` для рига с лимитом (CLI, `--config`, без демона)

```
$ loop-manager config explain --config …/config.toml --project shop --formula build --rig web
rounds = 3  # default
max_writers = 1  # rig
# step `1` of `build`: exec `true`

$ loop-manager config explain --config …/config.toml --project shop --formula build
rounds = 3  # default
# step `1` of `build`: exec `true`

$ loop-manager config check …   # max_writers = 0
…/shop/loop-manager.toml: TOML parse error at line 3, column 15
invalid value: integer `0`, expected a nonzero u32
```

## Нерешённое / известное

- Осторожная неточность: прогон с `continue_from` от worktree-прогона работает в старом worktree, но по формуле без
  `worktree` считается писателем.
- Снижение лимита reload'ом ниже числа живых: две семьи `drain` одного рига могут ждать друг друга до конца одной из
  них (причина видна, лечится лимитом или cancel) — кодом не решалось, по плану.
- Вопрос владельцу о `[root]` (сессии scope `root` в каталоге root) — остаётся за coder/журналом, я журнал не трогал.
- Guides (`docs/guide/first-formula.md` упоминает `waiting_for`) не правил — по плану.

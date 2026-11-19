# WP-137 · Сбой бида — только непоправимый провал · план expert

Цикл 133. Код и документы читаны на HEAD `832110b`.

## 0. Главное решение

**Счёт не меняется, меняются имя, определение в документах и тесты.** Сбой бида — прогон на нём, закончившийся
`failed`, пока он держал claim, любого класса. Ключ переименовывается в **`bead_failed_runs`**: имя называет то, что
считается — прогоны в состоянии `failed`, а не «сбои» вообще (раунд, красный чек, повтор шага).

Почему фильтра по классу или причине нет:

1. Слова владельца уже выполняются кодом. «Красные тесты на втором круге, возврат ревьюера, ещё один раунд» прогон не
   заканчивают: `let_go_bead` зовётся только из `terminal` (`run/machine.rs:387`), а возврат раундом и повтор шага
   идут через `enter` (`run/finish.rs:172`, `run/repeat.rs:99`). «Не хватило раундов или попыток» — это и есть
   `failed`.
2. Отличать «исчерпаны `rounds`/`max_attempts`» от прочих `failed{agent}`/`failed{check}` нечем и незачем — прочих
   нет. `repeat` (`run/repeat.rs:94-101`) завершает прогон только в трёх случаях: `fatal` (exit 75), попытки шага
   кончились и шаг не чек (`finish.rs:133-135`; `max_attempts` по умолчанию 1 — тоже «кончились»), попытки кончились
   у чека и раунды кончились (`finish.rs:164-167`). Признак в данных прогона не нужен, reason не разбирается.
3. Класс `infrastructure` контракт сам определяет как «attempts ran out or the policy ended the run»
   (`docs/design/contracts.md:694`), решение 122 называет такой прогон «failed for good». Восстановимое чинится
   внутри прогона (`resume`, `new_session`, `park`), до `failed` доходит то, что не починилось.
4. Не считать `infrastructure` небезопасно. Счётчик — единственный тормоз петли «бид освобождён → `bead.ready` →
   триггер → новый прогон» (`docs/design/tracker-br.md:62`, `docs/design/config.md:890`). Часть причин этого класса
   детерминирована и срабатывает за миллисекунды: `every step was skipped`, ошибка рендера шаблона,
   `config_error: check script: …`, `config_error` exec-шага (у него нет ключа агента, политика даёт `fail` без
   гейта), `no_decision` без политики. Без счёта такой бид крутится вечно. Отсечь их можно только разбором reason
   или вторым счётчиком с потолком ядра — новый механизм, которого владелец не просил.

Это допущение expert, а не слова владельца: в решение 147 идёт только сказанное владельцем, классификация — в
`assumptions.md` и вопросом в журнал (раздел 5).

## 1. Исходы прогона

Условия счёта — `run/beads.rs:236-283`: бид у прогона есть (`:237`), прогон стартовал и ещё владелец (`:245`),
claim вернул бид (`bead_data`, `:265`), состояние `failed`.

### Считается (+1)

| Исход | Где получается | Класс, reason | Почему сбой |
|---|---|---|---|
| Раунды кончились | `finish.rs:164-167` | `agent`, `still failing after <n> rounds: …` | названо владельцем |
| Попытки шага кончились, шаг не чек | `repeat.rs:101` → `finish.rs:133-135` | `agent` (шаг сказал `fail`, exec вышел не 0, drain остановлен) или `check` (скрипт проверки отказал); reason — summary исхода | названо владельцем |
| Exit 75 скрипта проверки | `repeat.rs:59`, `:96` | `check`, `check script: exit 75` | скрипт сам сказал «насовсем» |
| Вето `run.finishing`, срок хуков | `finish.rs:108`, `decide.rs:84` | `check`, причина хука или `hook_deadline` | отказ без возврата |
| Deny `run.preparing`, срок хуков | `decide.rs:83`, `:104` | `check` | отказ без возврата; детерминирован |
| Потолок 10 замечаний | `finish.rs:278-280` | `check` или `hollow_done`, `remarks_ceiling` | кончились попытки (инвариант 16) |
| Ход без исхода | `decide.rs:131-134` | `hollow_done`, `no_decision` или причина политики | политика сдалась (2-й раз) |
| Срок `await` | `await_step.rs:58` | `check`, `await_timeout` | ждать больше нечего |
| Политика завершила прогон | `decide.rs:105-109` | `infrastructure`, `no_decision` или причина `fail` (сюда же exit 75 exec-команды: `exec_step.rs:134` → `unknown` → 3 `resume` → `fail`) | попытки политики кончились |
| Потолок 20 сбойных попыток | `decide.rs:165-167` | `infrastructure`, `attempts_ceiling` | кончились попытки (инвариант 16) |
| Все шаги пропущены | `finish.rs:195-203` | `infrastructure`, `every step was skipped` / `every step of round <n> was skipped` | детерминировано, новый прогон не поможет |
| Скрипт проверки не стартовал | `repeat.rs:42-45`, `:69-70` | `infrastructure`, `check script: …` / `config_error: check script: …` | детерминировано |
| Прогон не может идти дальше | `machine.rs:46`, `:113-122`, `:166`, `:177`; `finish.rs:111`, `:125`, `:229`; `exec_step.rs:83`; `drain.rs:37`, `:86`; `formula_step.rs:26`, `:42`, `:46` | `infrastructure` (шаблон, снимок конфига, драйвер, workdir, шаг исчез, `outcome lost`, дочерний прогон отказан, `nothing_ready`) | детерминировано или потеря состояния |
| 7 дней в `parked` | `events.rs:291` | `infrastructure`, `park_timeout` | срок ожидания вышел |

### Не считается

| Исход | Где | Почему | Чем отсекается |
|---|---|---|---|
| `passed` | `finish.rs:192` | успех; счётчик сбрасывается в 0 | `beads.rs:257-263`, `:270` |
| `cancelled` (оператор, `parent_cancelled`, `parent_failed`, `drain_hard_stop`) | `events.rs:29` | чьё-то решение, не исход работы | `beads.rs:266` |
| Не стартовал: `queue_timeout` | `events.rs:290` | claim не запрашивался | `beads.rs:245` |
| Claim не ответил: `tracker_unavailable` | `events.rs:292` | о биде ничего не известно | `beads.rs:265` |
| Claim отклонён: `bead_claimed`, `bead_not_found`, `bead_invalid: …` | `beads.rs:182-200` | бид не был нашим | `beads.rs:196-199` → `:245` |
| Бид чужого рига: `bead_of_another_rig` | `beads.rs:174` | прогон на биде не работал (`bead_data` ещё не записан) | `beads.rs:265` |
| Корневой бид не создан: `bead_not_created: …` | `drain.rs:218`, `:259`, `:264` | бида у прогона нет | `beads.rs:237` |
| Внутри живого прогона: сбойная попытка с `resume`/`new_session`/`park`, повтор шага, возврат раундом, remark | `decide.rs:110-130`, `repeat.rs:97-100`, `finish.rs:168-172` | прогон не закончился | `let_go_bead` не вызывается |

Событие закрытия гейта не меняется: `gate.closed {gate: {key: "bead:<project>/<id>", closed_by: "core", reason:
"<n> failed runs on the bead"}}` (`beads.rs:277-282`). Имени ключа в нём нет, reason уже говорит «failed runs».
Колонка `beads.failures` остаётся (внутреннее имя, схема не трогается). `config explain` ключей оркестратора не
печатает (только эффективный конфиг агента) — править там нечего.

## 2. Имя ключа

- Новое: `bead_failed_runs` (оркестратор, по умолчанию 3, не меньше 1). Умолчание не меняется.
- Старое: строка в `RETIRED` (`crates/loop-manager-contract/src/config/retired.rs`, массив 6 → 7):
  `("unknown field `bead_failures`", "`bead_failures` is `bead_failed_runs` now")`.
  Пользователь увидит `line 1, column 1: `bead_failures` is `bead_failed_runs` now`. Алиаса нет (решения 122, 140).

## 3. Правки

### Код

| Файл | Что |
|---|---|
| `crates/loop-manager-contract/src/config/files.rs:177` | поле `bead_failed_runs` |
| `crates/loop-manager-contract/src/config/load.rs:28-29`, `:98` | `DEFAULT_BEAD_FAILED_RUNS`; rustdoc: `Runs of one bead that may end `failed` before the core closes its gate` |
| `crates/loop-manager-contract/src/config/mod.rs:40-41` | поле `Config::bead_failed_runs`, тот же rustdoc |
| `crates/loop-manager-contract/src/config/retired.rs` | строка из раздела 2 |
| `crates/loop-manager-core/src/run/beads.rs:233-235`, `:264`, `:274` | имя поля; rustdoc `let_go_bead`: считается любой `failed`, ссылка на `contracts.md 6.2` (сейчас ошибочно «5»); комментарий у `:264` — «A failed run of any class counts: a run fails only for good. Only a run that held the claim: …» |

Логика `let_go_bead` не меняется. Бюджет ядра: +4 строки.

### Тесты

1. `crates/loop-manager-contract/src/tests/config_launch.rs`, `a_retired_key_is_one_line_that_says_where_it_went`:
   строка `("config.toml", "bead_failures = 2\n", "`bead_failures` is `bead_failed_runs` now")`.
2. `crates/loop-manager-testkit/tests/common/beads.rs`: перенести сюда `finish_bead`/`finish_attempt` из
   `tests/core/beads.rs:15-34` как `pub(crate)` (тот файл — 394 строки из 400).
3. Новый модуль `crates/loop-manager-testkit/tests/core/bead_failed_runs.rs` + `mod bead_failed_runs;` в `main.rs`
   после `bead_events` (решение 115). Мир: `FIX` + `POLICY[1]` + свой `config.toml` =
   `POLICY[0]` с `bead_failed_runs = 1` второй строкой. При пороге 1 «считается» = есть `gate.closed` с ключом
   `bead:shop/<bead>`, «не считается» = его нет. Хелпер `gated(&w, &bead) -> bool` по `w.all("gate.closed")`.

| Тест | Как | Проверяет |
|---|---|---|
| `a_run_out_of_rounds_is_one_failed_run_and_a_round_back_is_none` | формула: `code` и `review` (оба `ai`, `review` с `back_to = "code"`), `rounds = 2`; 4 `slow_turn`; исходы попыток pass, fail, pass, fail. `start_paused` | после 2-й попытки гейта нет, после 4-й `failed{agent}`, reason с `still failing after 2 rounds`, ровно один `gate.closed` |
| `a_step_out_of_attempts_is_one_failed_run_and_a_repeat_is_none` | один `ai`-шаг, `max_attempts = 2`; fail, fail. `start_paused` | после 1-й гейта нет, после 2-й `failed{agent}` и гейт |
| `exit_75_of_a_check_script_is_a_failed_run_of_the_bead` | `fix` с `step_check_script_path = "check.sh"`, `max_attempts = 3`, скрипт `#!/bin/sh\nexit 75\n`; `repeat_turn`/`repeat_declare`/`repeat_end` из `step_repeat.rs`. Реальный процесс → `#[tokio::test]` без `start_paused` | `failed{check}`, гейт |
| `an_attempt_the_policy_retries_is_no_failed_run_and_the_run_it_gives_up_is_one` | скрипт `Normalized::Failure(HarnessCrash)`; `decide(.., "run.attempt_failed", 0, NewSession)` → гейта нет; второй сбой без решения, сон за срок хука (10 с) | `failed{infrastructure, no_decision}`, гейт |
| `a_cancelled_run_releases_its_bead_without_counting_a_failure` | перенести из `core/beads.rs:253-270` (имя сохранить), добавить `assert!(w.all("gate.closed").is_empty())` | сейчас тест счёт не проверяет вовсе |
| `a_passed_run_resets_the_failed_runs_of_its_bead` | свой порог 2. Бид `E` готов → прогон `hollow_turn` (failed) → `w.tracker.add(&[], Some(&E))` (с открытым ребёнком `close` в `MemoryTracker` освобождает, а не закрывает) → прогон pass → прогон failed: гейта нет → ещё failed: гейт | сброс на `passed` (`beads.rs:270`), сейчас без теста |

   Уже есть и остаются: `hollow_done` и умолчание 3, сброс открытием гейта —
   `failed_runs_release_the_bead_and_the_third_closes_its_gate_until_the_operator_opens_it`; claim без ответа —
   `claims_that_answer_after_their_runs_failed_neither_count_nor_reach_the_next_run` (оба `core/beads.rs`).
   Имён этих тестов в фильтрах `acceptance/src/checks.rs` и `mise.toml` нет — перенос фильтры не ломает.
4. `crates/loop-manager-acceptance/src/scenarios/c16_beads.rs`: `:4` и `:133` — имя в комментариях, `:50` —
   `bead_failed_runs = 2`, `:177` — `out after 2 failed runs`. Проверка — `cargo clippy --workspace --all-targets`.
   Сценарий не запускается (так сказал coder; `rules-tests.md` разрешает прогон одного правленого критерия, но
   без просьбы владельца не нужно). В отчёте отметить: критерий 16 правлен и не прогонялся.

### Документы

| Файл | Правка |
|---|---|
| `docs/design/config.md:336` | `bead_failed_runs = 3                # runs of one bead that may end `failed` before the core closes its gate (default 3, at least 1)` |
| `docs/design/config.md:20-22` | в список retired: `` `bead_failures` (`bead_failed_runs`) `` |
| `docs/design/config.md:890-891` | `Repeated runs over one bead are bounded by its failed runs (`bead_failed_runs`).` |
| `docs/design/contracts.md:795-797` (это 6.2, не 5) | текст ниже |
| `docs/design/tracker-br.md:59-62` | `The failed runs of a bead are counted in the state of the orchestrator: every run that ended `failed` while it held the claim is one, of any class; a round or a repeat inside a run is none, nor is a cancelled run ([contracts.md 6.2](contracts.md#62-transitions)). `passed` resets the count. At `bead_failed_runs` (orchestrator config, default 3) the core closes the gate …` — хвост как был |
| `docs/guide/recovery.md:34-35` | `… and its count of failed runs went up by one (a passed run resets it). At `bead_failed_runs` failed runs (default 3) loop-manager closes the gate` |
| `plugin/skills/recover/SKILL.md:52-53`, `:71` | `… and its count of failed runs went up. Once the count reaches `bead_failed_runs` (default 3) …`; `:71` — `(that resets the count)` |
| `docs/acceptance.md:57` | `after the set number of failed runs the bead is given out no more` |
| `devloop/README.md:29`, `devloop/formulas/task.toml:5` | только имя ключа; пак переделывает WP-142 |
| `docs/agents/decisions.md`, раздел Tracker, после строки 96 | решение 147, текст ниже |
| `docs/agents/assumptions.md:108` | текст ниже |

`contracts.md` 6.2, вместо «The failure counter of the bead: … resets the counter.»:

> The failed runs of a bead are counted: a run that ends `failed` while it held the claim is one, whatever its class,
> because a run fails only for good: the rounds of its formula or the `max_attempts` of a step ran out, a check
> refused it outright (exit 75, a veto, a deny), the policy or a ceiling of the core ended it (6.1), or it could not
> go on. What a live run goes through is not counted: a failed attempt the policy retries, a repeat of a step, a
> check that sends the run back for another round. `passed` resets the count; `cancelled`, and a run whose claim was
> refused or never answered, leave it. At `bead_failed_runs` (orchestrator config, default 3) the core closes the
> gate `bead:<project>/<id>`, and the operator opening that gate resets the count.

Решение 147 (только слова владельца):

> | 147 | A failure of a bead is one that cannot be repaired: the rounds or the attempts for the work ran out. Red tests on a second round, a reviewer sending the work back, one more round are not failures. The key that bounds the failures of a bead says so by its name and by what it counts: `bead_failed_runs`, the runs of the bead that ended `failed` (2026-11-19) |

`assumptions.md:108`:

> - Every run that ended `failed` while it held the claim is a failed run of its bead, of any class, `infrastructure`
>   among them: a run fails only when the recovery inside it is spent. After `bead_failed_runs` of them (default 3)
>   the core closes that bead's gate until a human opens it.

`demos/4-context-budget/library-example/GLOSSARY.md:127` упоминает `bead_failures` — это результат прогона демо,
не конфиг; не трогать, загрузку ничего не ломает. В отчёте отметить.

## 4. Риски и мутации

Риски:
- Владелец мог ждать, что `infrastructure` перестанет считаться. Принято обратное; вопрос — в журнал (раздел 5).
- Реальный тест с exit 75 идёт на настоящем времени: ждать события (`repeat_end`), не сна.
- Шаблон `fixer` в формуле с `back_to`: если загрузка требует `{{ remarks }}` в шаблоне шага, куда возвращаются, —
  дать тесту свой шаблон, как в `returns.rs:199-215`.
- Строка конфига в `c16_beads.rs` проверяется только прогоном сценария: после правки
  `grep -rn bead_failures crates/ docs/ plugin/ devloop/` должен находить только `retired.rs`, его тест и список
  retired в `config.md`.
- Заметка для WP-142, не для этого пакета: «кончились раунды — ждём человека» получается при
  `bead_failed_runs = 1` в `devloop/config.toml`; при 3 бид получит три прогона по 50 раундов.

Мутации (каждая должна ронять названный тест):

| Мутация | Тест |
|---|---|
| `beads.rs:265`: `"failed" \| "cancelled"` | `a_cancelled_run_releases_…` |
| `beads.rs:265`: добавить `&& row.class.as_deref() != Some("infrastructure")` (затем `"check"`, `"agent"`) | тест политики; exit 75; раунды и `max_attempts` |
| `beads.rs:265`: убрать `if row.bead_data.is_some()` | `claims_that_answer_after_…` |
| `beads.rs:270`: `failures + ?3` без `* ?3` | `a_passed_run_resets_…` |
| `beads.rs:274`: `<` → `<=` | все тесты с порогом 1 |
| `load.rs:98`: всегда умолчание | все тесты с порогом 1 |
| `load.rs:29`: 3 → 4 | `failed_runs_release_the_bead_and_the_third_closes_…` |
| `retired.rs`: убрать новую строку | `a_retired_key_is_one_line_…` |

## 5. Запись в журнал (текст для coder)

```
## <дата> · цикл 133 · coder · РЕШЕНИЕ

WP-137: ключ оркестратора `bead_failures` переименован в `bead_failed_runs` (старое имя — ошибка загрузки с
подсказкой). Что считается, не изменилось и теперь записано явно: сбой бида — любой прогон, закончившийся `failed`,
пока держал claim. Возврат раундом, повтор шага и попытка, которую политика перезапускает, прогон не заканчивают и
никогда не считались; `cancelled` и прогон без полученного claim не считаются. Допущение в
`docs/agents/assumptions.md` (Tracker) переписано, решение владельца — строка 147 в `docs/agents/decisions.md`.

## <дата> · цикл 133 · coder · ВОПРОС

Считать ли сбоем бида прогон, который упал по инфраструктуре (`failed{infrastructure}`: политика сдалась после трёх
зависаний подряд, 20 сбойных попыток, ошибка конфига, все шаги пропущены)? На кону: при пороге 1 такой прогон
закрывает гейт бида и ждёт человека, хотя задача не виновата. Варианты: (1) считать, как сейчас, — принято до
ответа: контракт определяет этот класс как «попытки кончились или политика завершила прогон», а счётчик —
единственный тормоз петли «бид освобождён → `bead.ready` → новый прогон», и часть таких причин повторяется
мгновенно и вечно; (2) не считать и добавить второй тормоз — потолок ядра на число любых упавших прогонов бида или
гейт с отсрочкой; это новый механизм, отдельный пакет.
```

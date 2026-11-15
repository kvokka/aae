# Цикл 79 — coder
STATUS: done

## Взято в работу
- Обязательные 1–3 ревью 78; желательные 1, 3, 4.
- WP-67 (корневой бид и `drain`, критичный, M): план у `expert` — `.loop/cycles/79/notes/wp67-plan.md`.

## Сделано (коммиты)
- `79808d0` — обяз. 1–3, жел. 1 и 4:
  - `invariants.md`: номера инвариантов стабильны, не перенумеровываются и не переиспользуются; строка таблицы
    `78/notes/spec-dedupe.md` поправлена.
  - READINESS: критерий 24 добавлен в «Recovery» и «br adapter», оба опущены до 6.
  - `steps.rs::the_attempts_ceiling_counts_no_passed_step`: шаг `a` проходит, `b` 19 раз выходит с 75, на 20-й проходит →
    `passed`. Мутация `decide.rs` (условие на `failure` убрано) — красный `run.failed{infrastructure, attempts_ceiling}`.
  - `steps.rs::a_resumed_step_never_takes_the_session_of_an_earlier_step`: шаг `two` стартует, сессия не приходит
    (`stall_sessions`), `kill`, политика `resume` → новая сессия, не сессия шага `one`. Мутация `machine.rs:127`
    (`step = ?2` → `?2 IS NOT NULL`) — красный детерминированно.
  - `pass()` в `steps.rs` ждёт токен не дольше 10 с.
- `1499347` — жел. 3: `config.md` «Loading»: снимок конфига — у шага; следующий шаг берёт конфиг на момент своей подготовки.
- `2e31fcc` — жел. 7: повтор имени шага — одна ошибка конфига, не две (тест в `config_steps.rs`).
- WP-67, план `expert` — `.loop/cycles/79/notes/wp67-plan.md` (5 коммитов). Сделано:
  - `1c28e3b` (subcoder) — коммит 1 плана: операция трекера `create(actor, key, title, description)`, один бид на ключ
    (br: эпик с `--external-ref`, поиск по списку; MemoryTracker); `close` родителя с открытыми детьми (br код 3) —
    release. Контрактный тест на настоящем br; мутации (без поиска по `external_ref`, без ветки кода 3) — красные.
  - `0319920` — коммит 3 плана: метки бида `agent:`/`harness:` (миграция 19, `runs.as_agent`, `Config::for_run_as`).
    Тест `beads.rs::a_bead_names_the_agent_of_its_run_and_the_harness_it_is_for`; мутации (`as_agent` не передан в
    `advance`; `!=` → `==` в сверке харнесса) — красные.
  - `31bd5f5` (subcoder) — коммит 2 плана: вход `{ request = "<текст>" }` → корневой бид-эпик в `preparing`, ключ в
    payload эффекта, повтор после рестарта дубля не даёт; `run/drain.rs`, тесты `tests/drain.rs`; мутации (новый ключ
    на каждое исполнение; снято «`row.bead` пуст») — красные.
  - `803e19f` (subcoder + правка) — коммит 4 плана: шаг `drain` (`run/drain.rs`, миграция 20 `runs.step_kind`,
    `DRAINING` в recover/`blocker`/`workdir_busy`/выборке и — моя правка — в `status().active_runs`: плановый рестарт не
    ждёт прогон в шаге drain). Тесты `tests/drain_steps.rs` (7), конфиг — `config_steps.rs`. Мутации (условие «свежо»,
    отсев «этой попытки», три `NOT DRAINING`, провал ребёнка → провал шага, `sync` в `let_go_bead`, `active_runs`) —
    красные. Ядро 10 090 / 18 000.
  - `c0d75c7` (subcoder) — коммит 5 плана: хаос-стенд (формула `sweep` с `drain`, проверки D1–D3 в `chaos/drain.rs`,
    категория и вид убийства «посреди drain»; мутации recover/отсев/`alive` — красные D3/D1/D2) и строка §15
    `c24_drain.rs`: настоящие демон, br и `claude` на поддельном провайдере, `ship` = plan → drain(`work`) → report,
    биды A, B (блок. A, риг `api`), F (падает), H (чужой харнесс), три `kill -9`; 3 прогона подряд — pass, ~2,5 с;
    с мутацией recover строка 24 FAIL.
  - `3598d5b` — находка subcoder: `Remark` на `run.finishing` exec/drain-шага давал ложный `daemon_crash` (решение,
    присланное в ядро напрямую). Теперь — отказ «does not apply»; тест `steps.rs::a_remark_on_an_exec_step_is_refused`,
    мутация убита.
  - `a88f2b5` — READINESS: «Recovery» и «br adapter» снова 8 (критерий 24 со строкой), «Runs and finisher» — 6 до
    критерия 25; маркер `ver=3598d5b`. WP-67 в плане — `done`.
  - `7d188fa`, `565692f` — строка 11 ждёт 38 контрактных тестов (добавлен тест br `create`); маркер READINESS
    `ver=7d188fa` (первый прогон checks упал на этих двух строках: 11 и 22).
  - `37ceb67`, `dbc0af5` — `shortcut:` в `reload.rs` (агенты из меток не в отпечатках перезагрузки), ширина строк.

## Не получилось и почему
- Жел. 2 ревью 78 (`max_runs` агента на границе шага) — не брал; `shortcut:` в `finish.rs` остаётся.
- Ветка `bead_not_created` (трекер отказал в `create` не `Unavailable`) тестом не покрыта: MemoryTracker отдаёт только
  `Unavailable`.
- Известные пределы WP-67 (комментарии `shortcut:` / журнал): корень-сирота, если трекер ответил после `start_timeout`;
  ребёнок прошлой попытки drain, созданный в ту же миллисекунду, что и новая попытка, считается её; отпечатки
  перезагрузки не знают агентов из меток `agent:`.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего не начато сверх сделанного. Следующий шаг по плану — WP-68 (возврат с пределом раундов, S), затем WP-69.
## Проверки
`scripts/loop/checks 79` на HEAD `565692f`: RESULT pass. ci — 54 с, nextest 442/442 за 45,8 с (было 425 за 41,8 с;
+17 тестов: drain, drain_steps, steps, beads, хаос с drain). acceptance-ci — 24 pass / 0 fail / 9 not run (строка 24
новая, pass).

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1, 2, 3 — закрыты (`79808d0`).
- Жел. 1, 3, 4, 7 — закрыты. Жел. 2 (`max_runs` на границе шага), 5 (время набора), 6 (ширина строк спеки), 8 (переносы) — не трогал.
## Контекст на момент отчёта: ~215k

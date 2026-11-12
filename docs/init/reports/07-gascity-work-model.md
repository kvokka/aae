# 07 — Модель работы Gas City: formulas, molecules, wisps, convoys, orders, sling, hook, dispatch

Gas City @ f1f75f53c; пути без префикса — от `/home/ubuntu/proj/work/gascity`. `PACKS` = `/home/ubuntu/proj/work/gascity-packs`, `SRC` = `tmp/gc-alt-research/src`.
Пометки: **[П]** = ПРОВЕРЕНО, **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО.

## 1. Выводы за 60 секунд

- **Molecule как сущность действительно лишняя, и это уже признали все три проекта.** В bd это буквально «template epic + клон детей» (`SRC/beads/cmd/bd/mol.go:41-47`); Gas Town по умолчанию перестал материализовать шаги (root-only wisp, чеклист инлайном в промпт: `SRC/gastown/docs/concepts/molecules.md:17-25`); upstream gc планирует растворить её в «Run» (issue #1709). beads_rust молекулы и конвои исключил сознательно (`SRC/beads_rust/docs/porting/PLAN_TO_PORT_...md:540,1751`). **[П]**
- **Но «молекулы живут в bd» — уже неправда.** С 2026-03-19 (коммит `98ca7b172` «Port formula compilation engine from beads; remove MolCook from Store») gc сам компилирует формулу и сам создаёт биды; для инстанцирования `bd mol` из gc не вызывается (на split-городе `gc bd mol bond` ещё и прямо отклоняется: `cmd/gc/bd_split_allowlist.go:218-219`). `engdocs/architecture/life-of-a-molecule.md` (verified 2026-03-17) описывает мёртвую схему. **[П]**
- **То, что молекула *делает* (шаги как долговечные биды, раздаваемые разным агентам), владелец использует в полный рост.** В его двух паках: 20 разных адресатов `gc.run_target` (18 литеральных + 2 переменных), 7 check-циклов, 2 drain, 9 gate, 5 scope, 29 condition; 26 из 49 шагов с `gc.run_target` адресованы скрипт-агентам (runner, city-runner, tester), а не LLM. «Формула = один промпт» его реальные формулы не покрывает. **[П]**
- **Convoy — переусложнение как сущность, но не как идея «входной набор одного прогона».** Сверх «epic + дети» он даёт только не-эксклюзивное членство (`tracks`, бид сохраняет parent). Cross-store он НЕ умеет (`internal/convoy/membership.go:31-47`), поля `owner`/`notify`/`molecule` — без потребителей, а gc при этом *отказывается* слингать epic (`internal/sling/sling_core.go:2131-2133`). **[П]**
- **Каждый v2-sling чеканит синтетический convoy**, они текут (open issue #5984), владелец держит отдельный order-подметальщик (`PACKS/packs/servitors/orders/servitors-convoy-sweep.toml`) и потратил ~450 строк на shim «epic→convoy» (`PACKS/docs/new-pack-rules.md:162-163`). **[П]**
- **Модель работы — ~98k строк не-тест кода (≈10.5% gc) и ~229k строк тестов.** Самое тяжёлое — dispatch/control (20.7k), hook-очередь (13.5k), orders (12.5k), formula (12.3k). Это не «наследие»: dispatch написан после марта 2026; 135 из 212 коммитов в `internal/dispatch` — `fix`. **[П]**
- **Control beads = состояние workflow-движка, вынесенное в трекер.** Плюс — crash-recovery даром; минус — ~190 ключей `gc.*`, 15 значений `gc.kind`, служебные биды (5 авторских шагов → 7 бидов; один check-шаг → spec + iteration + control), карантины, cross-store gates (999 строк). **[П]**
- **Orders у владельца — это почти только `exec` + `cooldown`** (26 из 27 exec; 23 cooldown, 3 event, 1 cron; формульный order один, из bd-пака). И история их запусков — 34 384 из 34 837 строк city-стора (98.7%). **[П]**
- **«Epic + дети» не хватит в четырёх местах:** (1) `blocks` слеп к исходу — упавший шаг разблокирует следующий; (2) циклы (check/retry) — статическим деревом не выразить; (3) динамический fan-out по набору, известному только в рантайме; (4) у бида один parent. Всё решается ~5 правилами в оркестраторе, без отдельных сущностей. **[В]**
- **Принцип «primitive must become more useful as models improve»:** пошаговая материализация для одного агента — не проходит; маршрутизация шагов между ролями/моделями, параллельный fan-out, детерминированные проверки и human gate — проходят (это транспорт). **[В]**
- **H6: частично подтверждена.** Выкинуть сущности molecule/wisp/convoy/control-bead — да. Выкинуть граф шагов — нет, если владелец хочет сохранить свои же пайплайны.

## 2. Факты

### 2.1. Что есть что — по коду

```
order (TOML: trigger + exec|formula) ──tick──▶ exec: shell в контроллере, без агента
                                      └──────▶ formula ─┐
gc sling <target> <bead|formula> [--on f] ──────────────┤
                                                        ▼
formula (TOML) ─compile─▶ recipe ─instantiate─▶ биды в сторе:
   v1: root type=molecule + parent-child step-биды (или root-only wisp, phase=vapor)
   v2: root type=task, gc.kind=workflow, gc.input_convoy_id ─▶ convoy ─tracks─▶ биды-цели
       + step-биды (только blocks-рёбра, gc.run_target → gc.routed_to)
       + control-биды (retry|ralph|check|retry-eval|fanout|drain|scope-check|workflow-finalize)
step-бид  ─▶ work_query агента («hook») ─▶ gc hook --claim ─▶ сессия ─▶ close + gc.outcome
control-бид ─▶ агент control-dispatcher (`gc convoy control --serve`) ─▶ ProcessControl
drain ─▶ на каждый член input convoy: unit-convoy (1 член) + item-root (под-workflow)
```

| Понятие | Определение по коду | Источник |
|---|---|---|
| formula | TOML → `Recipe` (плоский список шагов + рёбра). Конвейер из 16 стадий | `docs/reference/specs/formula-spec-v2.md:48-62,512-519` **[П]** |
| molecule | v1: контейнер `type=molecule` + parent-child шаги; после cook «инертные данные», движка нет | `formula-spec-v2.md:76-82` **[П]** |
| wisp | эфемерная молекула (bd: `Ephemeral`, «not synced via git»); в gc — `gc.kind=wisp`, root-only при `phase="vapor"` без `pour` | `SRC/beads/internal/types/types.go:173`; `formula-spec-v2.md:187-188` **[П]** |
| workflow (v2) | `task`-root + шаги + control-биды; root закрывает `workflow-finalize` | `formula-spec-v2.md:53-72,528-540` **[П]** |
| convoy | бид `type=convoy`; членство — неблокирующее ребро `tracks`; terminal = closed/tombstone; только в одном сторе | `internal/convoy/convoy.go:51-56`, `membership.go:12,18-20,31-47` **[П]** |
| order | `trigger` ∈ cooldown/cron/condition/event/manual/webhook; `exec` (shell в контроллере) XOR `formula` | `internal/orders/order.go:26-40`, `triggers.go:128-138` **[П]** |
| sling | 4 режима: `--formula`, `--on`, default formula агента, plain bead; затем route + merge-метаданные + auto-convoy + poke + nudge | `internal/sling/sling_core.go:64-94,857-1049` **[П]** |
| hook (очередь) | `gc hook` исполняет `work_query` агента; `--claim` — атомарный claim одного бида | `gc hook --help` **[П]** |
| control dispatcher | встроенный **агент** с `start_command = gc convoy control --serve --follow`; будится событиями, fallback-sweep 1–5 с | `internal/bootstrap/packs/core/agents/control-dispatcher/agent.toml:2`; `cmd/gc/dispatch_runtime.go:53-77` **[П]** |

**Граница bd/gc.** bd даёт строки, зависимости, `ready`, флаг ephemeral, кастомные типы. Компиляция формул, инстанцирование, control-логика, convoy, sling, orders — всё в gc. В bd осталось ~10.4k строк `mol*/wisp/cook/pour/formula/template` + 5.8k `internal/formula`, которыми gc не пользуется **[П]** (`wc -l`, клон bd @ 5ac1941).

**v1 vs v2.** v1 — дерево, которое один агент проходит в своей сессии. v2 (первый коммит `ae78c3318`, 2026-03-20) появилась ради трёх вещей: шаги независимо маршрутизируются разным агентам; циклы и fan-out исполняет оркестратор вне сессии; scale-from-zero пулы видят шаги через `Ready()` (корень-контейнер v1 не Ready-visible: `formula-spec-v2.md:1268-1276`). Оба контракта живы; `gc converge` принимает только v1 (`:1251-1258`). **[П]**

### 2.2. Объём и сложность

| Понятие | не-тест / тест, строк | CLI-подкоманд | Сложность |
|---|---|---|---|
| formula (`internal/formula` + `cmd/gc/*formula*`) | 12 302 / 23 636 | 4 | 24 ключа шага, 15 top-level (таблицы спеки §1.2–1.3) |
| molecule + wisp | 8 076 / 18 895 | 0 (через sling/cook) | deferred-assignee, graph-apply, wisp GC |
| dispatch/control (+graphroute, graphv2, sourceworkflow) | 20 676 / 58 334 | 1 (`convoy control`) | 8 control-kinds, 4 structural, 6 состояний drain |
| convoy | 3 567 / 5 061 | 13 | 5 полей, owned/synthetic |
| orders | 12 505 / 33 533 | 7 | 6 триггеров, 3 gate-флага диспетчера |
| sling | 9 112 / 27 949 | 1, 16 флагов | 4 режима × single/batch × v1/v2 |
| hook-очередь (+workquery, readywork) | 13 506 / 32 044 | 3 | claim-окно, reclaim, adoption, drain-ack |
| convergence | 6 957 / 11 710 | 9 | отдельный цикл, только v1 |
| workladder / runproj / gates(cmd) | 1 916 / 7 317 / 2 061 | — | — |
| **Итого** | **≈98 000 / ≈229 000** | | |

Измерено `wc -l` по файлам **[П]**. Различных литералов `"gc.*"`: 190 в `internal/beadmeta`, 409 по всему не-тест коду **[П]** (`grep -rhoE '"gc\.[a-z_0-9.]+"' | sort -u | wc -l`).

**Мёртвая поверхность формул.** На 116 формулах (bundled + `gascity-packs-upstream`): `advice` 0, `pointcuts` 0, `compose` 0, `aspect` 0, `loop` 0, `waits_for` 0; `check` 36, `on_complete` 18, `drain` 13, `extends` 51, `retry` 2, `gate` 1 **[П]** (grep). Спека сама перечисляет «Accepted But Inert»: until-loop исполняется один раз, словарь gate-типов и режимы `waits_for` не читаются (`formula-spec-v2.md:1137-1165`). `tally` уже удалён из SDK (`:994-997`). **[П]**

### 2.3. Что реально использует владелец

- Формулы (`PACKS/packs/{metropolis,servitors}/formulas`, 15 файлов, все v2): конструкции — см. §1. `retry` 0, `on_complete` 0, convergence 0 реальных вызовов. **[П]**
- `metropolis.rig-task` — 646 строк, ~25 шагов, 3 check-цикла, 1 shared drain, 4 gate, scope + teardown. Новый пак `servitors` написан под бюджет «≤8 шагов на формулу» (`PACKS/docs/new-pack-rules.md:27`) и всё равно стоит на drain + check + gate + convoy. **[П]**
- Детерминированные шаги исполняют «агенты»-скрипты (`start_command`, `lifecycle = "one_shot"`: `PACKS/packs/servitors/agents/tester/agent.toml`, `metropolis/agents/{runner,city-runner}/agent.toml`). **[П]**
- Правило владельца №11: набор для fan-out «MUST NOT be replaced by labels, metadata or an epic's children: drain, the root dedupe, auto-close and retire-on-claim read only convoys» (`new-pack-rules.md:173-175`). То есть конвой обязателен **потому что машинерия gc читает только конвои**, а не по существу задачи. **[П]** + **[В]**
- Живой город: `gc bd count --by-type --include-infra` → HQ 34 837 строк, из них 16 convoy, 3 epic, 339 не-инфра; по меткам 34 384 `exec` (метка записи запуска exec-order: `internal/orders/store.go:40-46`). Риги: mp-api 409 (10 convoy, 6 epic, 30 spec), mp-front 111. **[П]**

### 2.4. Molecule/wisp: что решает и чем заменить

Реальные задачи: (а) шаг переживает смерть сессии и продолжается с середины; (б) шаги уходят разным ролям/моделям/провайдерам; (в) параллельные ветки; (г) аудит по шагам; (д) точка привязки для гейтов и проверок.

| | Вариант 1: формула = промпт, прогресс в notes одного бида | Вариант 2: epic + дети с deps средствами трекера | Вариант 3: граф в оркестраторе (durable workflow) |
|---|---|---|---|
| Возобновление | по notes + resume сессии; гранулярность — на совести агента | как сейчас (закрытые шаги закрыты) | журнал движка |
| Разные агенты на шаг | нет (только через субагентов/ручной sling) | да (поле agent на бид) | да |
| Параллелизм | только внутри одной сессии | да, через `ready` | да |
| Check/retry | агент проверяет сам себя | нужен код оркестратора (переоткрыть шаг, счётчик) | родное |
| Fan-out по набору | нет | cook-time — да; runtime-набор — нужен код | родное |
| Gate (человек) | «остановись и жди» в промпте | бид-блокер, закрывает человек | родное |
| Scope/abort | нет | нужен код: `blocks` удовлетворяется закрытием с любым исходом (`formula-spec-v2.md:935-936`) | родное |
| Наблюдаемость | один бид + транскрипт | bv/br показывают всё | нужен свой viewer (upstream написал `runproj`, 7.3k) |
| Что исчезает | step/control-биды, wisp GC, continuation groups | control/spec/finalize-биды, convoy | весь «workflow в трекере», 98% мусорных строк |

Вариант 1 — это ровно то, к чему пришёл Gas Town: «Formula steps are NOT materialized… Agents read steps inline», мотив — «~6,000+ rows/day → ~400/day», а `pour = true` оставлен для «if you would curse losing the progress after a crash» (`SRC/gastown/docs/concepts/molecules.md:18-25,110`) **[П]**. Для однорольной работы сильная модель 2026 года делает его достаточным **[В]**. Для пайплайна владельца (planner → reviewer другого провайдера → script → implementer×N → 5 QA-ревьюеров) — нет.

Тест «полезнее с ростом моделей»: пошаговая материализация для одного агента — **не проходит** (чем умнее модель, тем длиннее чеклист она держит сама). Step-биды как единица маршрутизации между агентами и convoy как «входной набор прогона» — **проходят** (больше параллелизма → нужнее координация). Control-биды — реализация, не примитив: тест к ним неприменим; `tally` upstream уже выкинул как judgment. **[В]**

### 2.5. Convoy против «epic + дети»

| Свойство convoy | Факт | Нужно ли при epic |
|---|---|---|
| Членство `tracks`, parent бида сохраняется | `sling_core.go:1022-1026` **[П]** | Да, если бид одновременно в продуктовом эпике и в наборе прогона → метка `run:<id>` или ребро `related` |
| Cross-store членство | **нет**: `ErrMemberNotCoResident` | Не теряется; владелец и так держит карту epic→rig tasks сам (`new-pack-rules.md:179-180`) |
| Динамическое добавление | `gc convoy add`, но drain строит manifest один раз (`new-pack-rules.md:533-534`) | Волны по `ready --epic` дают это честнее |
| Авто-закрытие + событие | `gc convoy check`, order `convoy-autoclose-backstop` (1h), `convoy.closed` | `br epic close-eligible` есть; событие — от оркестратора |
| `owner`/`notify`/`molecule` | только пишутся и печатаются; у `convoy.molecule` читателя нет (grep по `cmd`, `internal`, bundled packs) **[П]** | Нет |
| `merge`/`target` | наследуются детьми при sling | Метаданные эпика |
| owned / `land` | ручной lifecycle | Эпик не авто-закрывать |
| Вход v2: `gc.input_convoy_id`, dedupe, retire-on-claim | да | Заменяется «run привязан к epic id» |

От трекера требуется: `ready --epic <id>` (в br есть, рекурсивно), `epic close-eligible` (есть), типы рёбер помимо blocks/parent-child (`br dep add -t`: «blocks, parent-child, related, etc.») **[П]** (`br ... --help`, 0.6.0). Событий br не шлёт — «epic завершён» выводит оркестратор по своим же записям закрытия **[НП]** (детали br — в отчёте по H5).

Обоснование upstream'а (#1709, open): формула должна работать над набором как целым — параллельная реализация через drain, затем ревью *всего* результата одним шагом. Это обоснование *набора*, а не отдельного типа бида: эпик с детьми выражает то же. **[В]**

### 2.6. Orders и sling

**Orders.** `exec` исполняется контроллером без агента и без wisp (`order.go:29-31`); `formula` создаёт workflow и роутит в `pool`. Сохранить: cooldown, cron, event (`on = "bead.closed"`), manual; `exec` и `formula`; per-order `env`, `timeout`, override из city.toml. Не тащить: запись каждого запуска бидом (см. 98.7%), три gate-флага диспетчера (`idempotent`, `no_work_gate`, `reserved_dispatch` — лечат медленный стор), webhook. Из 27 активных orders 8 — подметальщики состояния самой системы (`wisp-compact`, `order-tracking-sweep`, `convoy-autoclose-backstop`, `gate-sweep`, `orphan-sweep`, `nudge-mail-sweep`, `servitors-convoy-sweep`, `metropolis-gate-sweep`) **[П]** (`gc order list`).

**Sling** сверх «назначить и разбудить»: выбор стора по префиксу и cross-rig guard; инстанцирование формулы (`--formula`/`--on`/default); слияние vars (rig `formula_vars`, target branch); идемпотентность и «один живой workflow на source-бид» с cross-process lock (`internal/sourceworkflow`, 1 025 строк); замена/откат при `--force`; auto-convoy с дедупом и reaping дубликатов (`sling_core.go:934-1035`); разворот convoy в детей; reopen для reassign; merge-стратегия. 9.1k строк — в основном произведение режимов (v1/v2 × single/batch × несколько сторов) и защита от повторного sling. **[П]** + **[В]**

### 2.7. Dispatch / control beads

Зачем бидами: состояние переживает рестарт контроллера без отдельного журнала; одна поверхность наблюдения; инвариант «SDK работает без пользовательских ролей» (`formula-spec-v2.md:708-714`). Цена: рантайм сам оговаривает «assumes a single controller… not a compare-and-swap guard» (`internal/dispatch/runtime.go:214-217`); воркер может сломать граф обычным `bd update` (`runtime.go:229-237`); каждое состояние — метаданные без схемы.

| Логика | Оценка |
|---|---|
| `workflow-finalize`, `scope-check` (упал член → skip остальных) | транспорт; политика «any-fail → fail» зашита (на это жалуется сам #1709) |
| `check` (exit 0 / 75 / иное) | транспорт: контракт кода возврата |
| Fallback по строкам stderr (`internal/convergence/gate_infra.go`) | эвристика; спека: «not a stable interface» (`:812-817`) — judgment в Go |
| `retry-eval`: нет outcome / нет output_json → `transient` (`internal/dispatch/retry.go:396-422`) | политика по умолчанию в Go; пограничное |
| `drain` / `fanout` | транспорт |

Альтернативы: обычный код движка с журналом (вариант C ниже) или агент-координатор (дешевле в коде, но судит LLM там, где достаточно `if`; владелец сам пишет «Mechanical steps run on a script agent, never on an LLM», `new-pack-rules.md:21`).

## 3. Вердикты

**H6 — ЧАСТИЧНО ПОДТВЕРЖДЕНА.**
- *Molecule/wisp как сущности* — лишние: подтверждено (§1, §2.4). В br их нет, портировать нечего.
- *Convoy как сущность* — переусложнение: подтверждено (§2.5). «Epic + дети + ready по эпику» достаточно с оговорками про единственного parent и cross-rig.
- *«Граф шагов не нужен, агент переварит»* — **опровергнуто собственными паками владельца**: 20 адресатов шагов, циклы с проверкой скриптом, fan-out, human gates. Что нужно сохранить — это способность, а не сущности.

**H7 — ЧАСТИЧНО (в части модели работы).** Из ~98k строк владелец не задействует: convergence (7.0k), v1-путь, `retry`, `on_complete`/fanout, advice/aspect/compose/loop/waits_for, webhook/condition-триггеры, convoy land/owned/target, почти все formula-orders. Но «наследие» — неточно: унаследован от bd компилятор формул (с мёртвыми аспектами), а самый тяжёлый слой (dispatch, sling, hook-claim) — новый код 2026 года, выросший из решения «workflow-состояние живёт в стороннем трекере, разложенном по нескольким сторам». **[В]**

**H8 — штрих.** Control dispatcher уже событийный (sweep-fallback 1–5 с). Тиковые — orders: 23 из 27 у владельца — cooldown-опрос. **[П]**

## 4. Минимальная модель работы для новой системы

Общее для всех вариантов: остаются **rig, agent, bead, epic, formula, order, event**. Исчезают как понятия: molecule, wisp, convoy, control bead, spec/finalize-биды, synthetic convoy, convergence, v1/v2. Sling сливается в «run formula on bead|epic» + «assign bead». Hook-очередь = `ready` с фильтром по agent. История orders — в журнал оркестратора, не в трекер.

**A. «Тонкий».** Формула = документ-чеклист для одной роли. Параллелизм только один: «N ready-детей эпика → N сессий пула». Циклы, ревью, гейты — словами в промпте; другая роль зовётся слингом дочернего бида. Плюс: минимум кода. Минус: нет принудительного бюджета раундов, нет изоляции ревьюера, script-шаги превращаются в tool-вызовы LLM.

**B. «Epic-граф» (база, которую стоит считать рекомендованной).** Формула материализуется в epic + дети + `blocks`. В оркестраторе — пять правил по событию закрытия бида: (1) ready-ребёнок → сессия агента из поля `agent`; (2) шаг `exec` исполняет сам оркестратор; (3) fail → остальные дети `skipped`, эпик `failed`, шаги `always` всё равно идут; (4) `check`/`retry` → тот же бид переоткрывается, счётчик в метаданных; (5) `for_each` → под-эпик на каждого ready-ребёнка целевого эпика, волнами до тишины. Метаданных — порядка десяти ключей.

**C. «Durable-граф».** Те же конструкции, но состояние прогона — в журнале оркестратора (SQLite), биды создаются только для настоящей работы и LLM-шагов. Run — первоклассная сущность (как в #1709). Чище трекер, быстрее, но нужен свой viewer и свой crash-recovery.

**Формулы владельца в новой модели**

1. `servitors.implement` (drain separate/exclusive):
   - A: формулы нет. «Sling epic → implementer pool»: сессия на каждого ready-ребёнка; эпик закрылся → event → order запускает QA.
   - B/C: `[[steps]] id="build" for_each="children" agent="implementer" session="fresh" exclusive=true`.
2. `servitors.plan-review` (check-цикл planner↔reviewer + опциональный gate):
   - A: промпт planner'у «напиши план, слингани review-бид ревьюеру, жди, правь ≤3 раундов». Бюджет раундов держит модель.
   - B/C: `plan(agent=planner)` → `review(agent=plan-reviewer, needs=plan)` с `loop = { back_to="plan", max=3, until="exec:check.py" }` → `approve(type=gate, assignee=human, when="approval==human")`.
3. `metropolis.quality-loop` (runner запускает lint/test, janitor/tester чинит, ≤4):
   - A: строка в промпте воркера «гоняй `mise run check && mise run test` до зелёного». Здесь рост моделей реально убирает движок.
   - B/C: `[[steps]] id="lint" exec="mise run check" on_fail={ agent="janitor", max=4 }` — один бид вместо трёх на итерацию; оставлять, только если починка сознательно отдана дешёвой модели.

**Что владелец недооценивает**
1. Свои же пайплайны: без варианта B он теряет plan-review, QA-lanes и implement.
2. `blocks` не различает pass/fail — нужна явная семантика провала.
3. Script-шаг должен быть первоклассным `exec`, а не фальшивым агентом с сессией.
4. Один parent у бида: «набор прогона» ≠ «продуктовый эпик».
5. Cross-rig эпик: дети в других сторах — ни convoy, ни epic этого не дают; нужна явная ссылка на уровне оркестратора.
6. «Подчистить, если что не так» — сегодня это 8 sweep-orders; в новой модели уборка должна быть следствием закрытия эпика, а не отдельными опросами.

## 5. Открытые вопросы и непроверенное

- **[НП]** Какая доля из 98k строк исполняется в живом городе — оценка по использованию конструкций, покрытие не снималось.
- **[НП]** Состав 34 384 `exec`-записей по времени и TTL (читал только агрегаты; `wisp_gc` и `order-tracking-sweep` включены).
- **[НП]** Содержимое #5984 дальше заголовка; актуальность строк, на которые ссылается `new-pack-rules.md` (сверены с gc `5ee67e887`, не с f1f75f53c) — перепроверил только `sling_core.go` (epic-отказ теперь на 2131-2133) и `membership.go`.
- **[НП]** Хватит ли br для правил варианта B (атомарный claim, метаданные на биде, типы `gate`) — вне темы, см. отчёт по H5.
- Решение за владельцем: B или C. Развилка — готов ли он отказаться от «всё видно в bv как биды» ради чистого трекера.
- `gc converge` (v1-only) владелец не использует; если нужен «итерировать до вердикта» — это `check` из варианта B.

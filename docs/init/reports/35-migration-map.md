# 35 — Карта переезда реального города владельца на новую систему

Дата: 2026-11-10. Всё про город и паки — только чтение. Эксперименты с `br` — в `lab/35-migration/` (изолированный `HOME`, явный
`BEADS_DIR`; LLM-запросов 0). Отчёты 02, 06, 07, 08, 11, 18 и дополнение к `WAVE1-SUMMARY.md` учтены.

Сокращения: `CITY` = `/home/ubuntu/proj/share/gc`; `PACKS` = `/home/ubuntu/proj/work/gascity-packs`;
`SV` = `PACKS/.claude/worktrees/servitors-kb/packs/servitors` (именно его импортирует город: `CITY/pack.toml:28-29`);
`MP` = `PACKS/packs/metropolis`; `RULES` = `PACKS/docs/new-pack-rules.md`; `LAB` = `tmp/gc-alt-research/lab/35-migration`.
Метки: **[П]** = ПРОВЕРЕНО, **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО. «Новая система» = демон-оркестратор, `<cli>` — его CLI.

## 0. Выводы за 60 секунд

- **servitors переезжает; ядро «5 правил» для него почти достаточно, но не совсем.** Не хватает четырёх вещей: gate как шаг, цикл
  над *группой* шагов (а не одним бидом), шаг-«дочерний прогон с ожиданием и таймаутом», исход `requeue` (отпустить бид без провала).
  Плюс примитив сессии «припарковаться до закрытия бидов и получить новый ход» (сегодня `gc session wait`). **[В]** из §3.
- **Главный блокер — не граф, а metadata.** Пак держит состояние в 20 своих ключах `servitors.*` и читает 25 ключей `gc.*`
  (`LAB/count_calls.py`, grep) **[П]**; в br metadata нет, а labels допускают только `[A-Za-z0-9_:-]` — путь, ветку, sha в label не
  положить **[П, эксперимент §6]**. Нужна KV-таблица «аннотации бида» в state оркестратора с CLI для скриптов пака.
- **Реальность толкает к варианту C, а не B.** Счётчик попыток, исход, причина отказа, вердикт раунда всё равно лежат в state:
  step-бид в br — лишь тень. В br — только настоящая работа (задача, task-биды плана, fix-биды), шаги — `run_step` в state,
  след — комментариями на F (пак так и пишет: 25 вызовов `bd comments add`). **[В]**
- **Объём правки пака.** Python без тестов — 3 514 строк: ~620 (18 %) исчезают, ~154 вызова `gc …` в 25 файлах переписываются
  на адаптер, ~2 100 строк (разбор плана, git-слияние, валидация финишеров, kb) переезжают как есть. Тексты для агентов: 124
  упоминания команд `gc` в 25 файлах, из них 52 — простое переименование финишеров. **[П]** подсчёт, **[В]** классификация.
- **Оба ордера servitors исчезают**, а с ними 8 переопределений и 2 `skip` встроенных ордеров в `city.toml`. Свои ордера в v0 паку
  не нужны вовсе. **[П]** `SV/orders/*.toml`, `CITY/city.toml:28-99`.
- **Служебные записи: ~85 бидов графа + 6–8 конвоев на сквозной прогон → 0 в трекере; фон 5 000 order-wisps/сутки → 0.**
  На прогон сегодня (снимок стора, `LAB/runs.py`): plan-review 9–14, explore 3, rig-task 43–81. **[П]/[В]**
- **Хоп: 22–39 с → 1–3 с** (LLM-шаг), <0,5 с (exec) **[В]**; старт процесса claude под оркестратором не измерен **[НП]**.
- **Данные почти не надо переносить.** Из ~970 durable-строк рабочих — 134–280, а не закрытых — **17** (hq 2, gcp 14, dot 1)
  **[П]** снимок `lab/06-storage`. Сырой `bd export` br не принимает; нужен трансформер (§6). История — архивом JSONL.
- **Именованные сессии `mayor` и `desk` определены в замороженном metropolis** (`CITY/pack.toml:34-42`), вместе с его диспетчером
  хуков на 4,1k строк. Нужен новый `mayor` (правило владельца №3: «desk нет»).

## 1. Инвентарь: что переезжает

### 1.1 Роли и сессии

Общее для всех 9 Claude-ролей servitors **[П]** `SV/agents/*/agent.toml`: `scope="rig"`, `provider="servitors-claude"`,
`wake_mode="fresh"`, `work_dir=".gc/agents/{{.Rig}}/<role>"`, `pre_start=['git init -q "{{.WorkDir}}"']`,
`env CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, плагин deja + MCP `kb`, фрагмент `servitors-kb` (`SV/pack.toml:55-56`).

| Роль | Харнесс / модель / effort | Pool | idle | Overlay → права и хуки | Skills / прочее |
|---|---|---|---|---|---|
| scout | claude haiku-5-5 / low | 2 | 15m | readonly: deny Edit/Write/NotebookEdit + 10 git-глаголов + `rm`; `replan` (PostToolUse); base: 11 deny, `deny-unbounded` | финишер `scout answer` |
| planner | claude opus-5-5 / high | 2 | 30m | readonly | `planner submit`; зовёт scout |
| plan-reviewer | claude opus-5-5 / high | 2 | 30m | readonly | `plan-reviewer verdict` |
| plan-reviewer-codex | codex gpt-5.6-terra / high, провайдер `servitors-codex` | 2 | 30m | readonly + `.codex/rules` (3 prefix_rule); хуки шестью `-c hooks.*` (`SV/pack.toml:36-50`) | промпт общий с plan-reviewer; **kb не получает** (gascity#9017); stall-sweep его не видит |
| implementer | claude sonnet-5-5 / high | 3 | 30m | implementer: deny `git worktree`; `stop-gate` (Stop); + `PYTHONDONTWRITEBYTECODE` | `implementer start/done/reset` |
| qa-reviewer-code / -test / -docs / -security | claude opus-5-5 / high | 4 | 30m | readonly | фрагмент `servitors-qa-review`; `qa-reviewer verdict` |
| qa-reviewer-dependency | claude sonnet-5-5 / high | 4 | 30m | readonly | то же |
| tester | **скрипт**: `start_command=python3 … session.py`, `lifecycle="one_shot"` | 20 | — | нет | шаги `run`, `gather` |
| mayor (named, `mode=always`) | claude sonnet-5-5 / medium, resume | 1 | — | `MP/assets/claude-lean`; `CLAUDE_CODE_DISABLE_CLAUDE_MDS`; хуки — диспетчер metropolis | адресат почты машин |
| desk (named, `mode=always`) | claude opus-5-5 / medium, resume | 1 | — | хуки metropolis (`desk-mail`, `inject-city`) | сессия оператора (`gc session attach desk`) |

Skills пака (4: scout, planner, implementer, tester) gc раздаёт **всем** Claude-агентам города (`SV/README.md:16-17`).

### 1.2 Формулы servitors (7, все v2) **[П]** `SV/formulas/*.toml`

| Формула | Шаги → адресат | Конструкции | Бидов на прогон сегодня¹ |
|---|---|---|---|
| explore | `explore` → scout | 2 vars, 1 шаг | 3 |
| plan | `plan` → planner; `approve` (gate) | `needs`, `condition`, `type="gate"`, `description_file` | 4 + конвой |
| plan-review | `loop`{`plan` → planner; `review` → plan-reviewer}; `approve` | `[steps.check]` exec, `max_attempts=5` литералом, enum-vars, gate | 9–14 + конвой |
| plan-crossreview | то же + `review-codex` → plan-reviewer-codex, параллельно | то же | 11 + конвой |
| implement | `build` — никто не claim-ит | `[steps.drain]` `separate` / `exclusive` | 2 + 4 на задачу [В] |
| implement-task | `work` → implementer | item-формула drain-а | — |
| qa | `loop`{`run` → tester; 5 × `review-*`; `gather` → tester} | check-петля, 5 `condition`, 9 vars, вложенный `gc sling` из `gather` | ~20 на раунд [В] |

¹ Измерено по снимку `lab/06-storage/issues_*.json` скриптом `LAB/runs.py` **[П]**; строки с [В] — оценка по структуре.
Не используются в servitors: scope/`abort_scope`, cleanup, `expand`, `continuation_group` — они есть только в `MP` (§3.6).

### 1.3 Ордера, хуки, MCP, сервисы, команды, doctor **[П]**

| Вид | Что | Где |
|---|---|---|
| Ордер | `servitors-convoy-sweep`: cooldown 30m, exec; закрывает синтетические input-конвои завершённых прогонов (gascity#5984) | `SV/orders/servitors-convoy-sweep.toml:6-12`, `assets/orders/convoy-sweep.py` (51) |
| Ордер | `servitors-stall-sweep`: cooldown 5m, exec; сессия молчит 30 мин по mtime транскрипта → `gc session close`, ≤3 раз, потом `deferred` + письмо mayor | `…stall-sweep.toml:5-10`, `stall-sweep.py` (102), `SV/config.toml:4-9` |
| Хук | `deny-unbounded` — PreToolUse/Bash, все роли (claude: settings.json; codex: `-c hooks.PreToolUse`) | `assets/overlays/base/.claude/` (76) |
| Хук | `stop-gate` — Stop, только implementer | `assets/overlays/implementer/.claude/` (63) |
| Хук | `replan` — PostToolUse/Bash, срабатывает на `gc hook --claim` | `assets/overlays/readonly/.claude/` (95) |
| Deny | base 11 правил (в т. ч. `Bash(gc bd update *)`, `Bash(gc bd close *)`), readonly 13, implementer 1; codex — `servitors-readonly.rules` | `assets/overlays/**` |
| MCP | `kb` — HTTP через прокси супервизора, всем агентам города | `SV/mcp/kb.template.toml:10-14` |
| MCP | `deja` — stdio на сессию: плагин Claude; для Codex `-c mcp_servers.deja` + 4 хука | `SV/pack.toml:45-49` |
| Сервис | `[[service]] kb`, `proxy_process`: `serve.py` (340) + `index.py` (292), дочерний `bm mcp` | `SV/pack.toml:62-68` |
| Команды | 10: `scout ask/answer/wait`, `planner submit/decide`, `plan-reviewer verdict`, `implementer start/done/reset`, `qa-reviewer verdict` (1 122 строки) | `SV/commands/*/*/run.py` |
| Doctor | `servitors:toon`, `servitors:kb` (160) | `SV/doctor/*` |
| Tunables | `config.toml` + городской оверрайд в `GC_PACK_STATE_DIR` | `SV/config.toml`, `SV/lib/config.py` |

### 1.4 Фактический API пака: вызовы `gc` / `bd`

Подсчёт `LAB/count_calls.py` по `SV` без тестов **[П]** (регулярки; вызовы через распакованный кортеж занижены на 1–2).

| Команда | Код (.py) | Тексты для агентов² |
|---|---|---|
| `gc bd update` | 29 | 3 |
| `gc bd comments add` / `comments` (чтение) | 25 / 6 | 2 / 9 |
| `gc bd show` | 19 | 15 |
| `gc bd list` (из них 9 — запросы `--metadata-field gc.*`) | 15 | 1 |
| `gc bd close` | 10 | — |
| `gc bd create` (2 — `--graph`) / `dep add,list` / `children` | 3 / 5 / 1 | — / 1 / — |
| `gc hook current` / `gc hook --claim` | 6 / 1 | — / 5 |
| `gc runtime drain-ack` | 7 | — |
| `gc convoy status/create/close/add` | 10 | 3 |
| `gc sling` | 2 | 17 |
| `gc mail send` | 4 | — |
| `gc session wait/close/list/peek` | 4 | 5 |
| `gc worktree ensure/verify/cleanup` | 4 | — |
| `gc rig list` | 3 | 3 |
| `gc servitors …` (финишеры и входы) | — | 52 |
| **Итого** | **~154** | **124** |

² `agents/`, `skills/`, `template-fragments/`, `formulas/`, брифы шагов — 25 файлов.
Плюс 12 env-переменных `GC_*` (`GC_BIN`, `GC_CITY`, `GC_RIG_ROOT`, `GC_SESSION_ID`, `GC_PACK_STATE_DIR`, `GC_BEAD_ID`, …) **[П]** grep.

## 2. Карта «было → станет»

Легенда: (=) как есть; (~) механически; (−) исчезает; (!) нужна новая возможность или решение.

| Позиция | | Станет / почему |
|---|---|---|
| Модель/effort/pool/idle ролей | = | поля agent; `idle_timeout` превращается в лимит тишины хода |
| `provider="servitors-claude"`, `servitors-codex` (TOML-в-TOML) | − | провайдеры-носители env/хуков не нужны: harness profile → agent |
| `overlay_dir` + симлинки base ← readonly/implementer, тест «не копия» | − | слои конфига сводит оркестратор; настройки на сессию (`_meta.claudeCode.options.settings`, override треда codex) |
| `pre_start = git init`, два settings-файла | − | нет файлов настроек в workdir |
| `work_dir` роли вне рига | ~ | cwd сессии выбирается до старта; для implementer — сразу worktree (§3.4) |
| `CLAUDE_CODE_DISABLE_CLAUDE_MDS`, `project_root_markers=[]`, `--disable plugins` | ~ | `inherit_user_config=false` в профиле (отчёт 18 §4.3) |
| `notice.model_migrations`, ответы на диалоги trust / «Hooks need review» | − | нет TUI; для codex — `bypass_hook_trust` |
| 3 хука | ~ | один `[[hook]]` на роль в диалекте Claude; пути `$CLAUDE_PROJECT_DIR` / `$GC_DIR` → каталог пака |
| `replan`: триггер по `gc hook --claim` | ! | claim исчезает → контекст re-plan вычисляет exec-шаг перед LLM-шагом (пишет в бриф); хук уходит |
| deny `Bash(gc bd update/close *)` | ~ | `Bash(br update *)`, `Bash(br close *)`; то же в `.codex/rules` |
| `gc hook --claim --drain-ack` в 5 промптах, `gc runtime drain-ack` ×7, `gc hook current` ×6 | − / ~ | push: бид приходит в промпте и в env `BEAD_ID`; конец хода = конец шага |
| tester как script-агент: `session.py` (75), heartbeat в панель | − | `exec`-шаги (правило 2) |
| `check.py` ×2 (95) | ~ | выражение `until` над результатами членов раунда |
| input-конвой, `end_run`, `convoy status` → «какой бид цель» (10 вызовов + обвязка) | − | цель прогона — поле `run.target` |
| `convoy create` для задач плана и fix-бидов | − | набор = открытые дети F; ad-hoc набор мэра — эпик (§3.4) |
| `claimed()`: разбор `gc.drain_manifest.v1` (44 строки) | ~ | `<cli> current --json` → {bead, run, target, siblings} |
| `stop()`: fail-fast в separate drain (41 строка) | ~ | правило 3 делает skip; остаётся только текст post-mortem |
| «последняя lane решает», гонка двух lane на `create --graph`, маркер `BY` | − | решение и `materialize` — после закрытия всех членов, один раз, exec-шагом |
| `bd create --graph` (атомарно) | ! | в br транзакций нет → адаптер: создать в статусе `draft`, связать, открыть |
| ID `<task>.<run><digest>.<n>` как ключ идемпотентности | ! | br такие ID отвергает **[П §6]** → идемпотентность по KV «digest плана» |
| `--set-metadata servitors.*` (20 ключей), чтение `gc.*` (25 ключей) | ! | KV-аннотации бида + результат шага в state; `<cli> kv get/set`, `<cli> step done --outcome --set k=v` |
| `gc session wait --on-beads [--sleep]` (`scout ask/wait`) | ! | примитив «припарковать сессию до закрытия бидов → новый ход в ту же сессию» |
| `implementer start`: «файлы заняты» → `dep add` + отпустить claim | ! | исход `requeue --blocked-by <bead>` |
| `gc sling … --on …` (вход оператора, вложенный запуск из `gather`) | ~ / ! | `<cli> run <formula> --on <bead>`; вложенный — шаг `run=` (§3.5) |
| `gc worktree ensure/verify/cleanup` (4) | ~ | обычный `git worktree` (в `reset` пак уже так делает) |
| `gc mail send mayor … --dedup` (4) | ~ | уведомление оператору с ключом dedup; тексты post-mortem остаются |
| `servitors-convoy-sweep` | − | нет конвоев |
| `servitors-stall-sweep` | − | liveness по событиям хода; «ждёт лимита» ≠ «завис» (сегодня дыра: `SV/README.md:256-263`) |
| 8 `[[orders.overrides]]` + `skip` в `city.toml` | − | все про Dolt, order-tracking и гигиену виспов |
| `[[service]] kb` + `${GC_CITY_NAME}` в URL + deadline 5 с из-за lock контроллера | ~ | сервис под супервизором, URL подставляет реестр MCP; codex получает kb |
| `deja` | = | stdio на сессию (шарить невыгодно, отчёт 18) |
| pack-команды `gc servitors …` | ~ | исполняемые файлы пака в PATH сессии (например `sv planner submit`) |
| doctor ×2, `config.toml` | = / ~ | `GC_PACK_STATE_DIR` → каталог состояния пака |
| `mayor`, `desk` из metropolis | ! | новый `mayor` в городе/servitors; `desk` упразднить |

**Строки** (`wc -l`, без тестов и docs; классификация — **[В]**):

| Что | Строк | Судьба |
|---|---|---|
| `assets/orders/*.py` + toml | 175 | − |
| `tester/session.py`, `check.py` ×2 | 170 | − (остаются 2 выражения) |
| конвои, drain-manifest, `stop()`, гонки lane, `scout wait`, сокет/deadline в `serve.py` | ~275 | − |
| **Исчезает Python** | **~620 из 3 514 (18 %)** | |
| `pack.toml` провайдеры, `agent.toml` (`provider`, `overlay_dir`, `pre_start`, комментарии про gc), settings-оверлеи | ~250 из 430 TOML/JSON | − / ~ |
| Формулы | 400 | переписать целиком: ~150 строк в новом синтаксисе |
| Вызовы `gc(…)` | ~154 в 25 файлах | ~ на адаптер; ~25 мест с KV — содержательно |
| План-парсер, git-слияние, валидация финишеров, kb, doctor | ~2 100 | = |
| Тесты (4 578), `fakegc.py` | — | фейк заменить; ~40 % тестов трогают конвои и `gc.*` [В] |
| Промпты, skills, фрагменты, брифы | 124 упоминания, 25 файлов | 52 — rename; `claim`/`sling`/`wait`/`convoy` (~30) — по смыслу |

metropolis (26 088 строк Python, 34 900 тестов) **не переносится**: заморожен самим владельцем.

## 3. Формулы servitors в новой модели

Напоминание — 5 правил движка (отчёт 07 §4 B): (1) ready-шаг → сессия агента; (2) `exec` исполняет оркестратор; (3) fail →
остальные skipped, `always` идут; (4) check/retry → повтор со счётчиком; (5) for_each → под-эпик на ready-ребёнка, волнами.
Ниже псевдо-TOML — иллюстрация, не синтаксис продукта. «B» = шаги как дети эпика в br; «C» = шаги в state.

### 3.1 explore

```toml
[[steps]] id="explore"  agent="scout"  session="fresh"  prompt="{{question}}"
```

- Правило 1. B: один бид-вопрос; C: 0 бидов, ответ — комментарий на `for_bead`.
- Промпт агента: шаблон роли + фрагмент kb + вопрос + «заверши: `sv scout answer`». Шага «Claim» нет.
- **Не хватает:** спрашивающий (planner) ждёт ответа; сегодня его паркует `scout wait` (`commands/scout/wait/run.py`). Нужен
  примитив `<cli> wait --on <ids>`: ход кончается, сессия остаётся привязана к шагу, по закрытию последнего бида оркестратор шлёт
  ей новый ход (resume, если процесс остановлен). Это свойство сессии, а не правило графа.

### 3.2 plan

```toml
[[steps]] id="plan"        agent="planner"
[[steps]] id="approve"     type="gate" by="human" needs=["plan"] when="approval == human" timeout="72h"
[[steps]] id="materialize" exec="sv planner materialize" needs=["plan","approve"]
```

- Правила 1, 2, 3. Финишер `submit` валидирует план, пишет `design` и комментарий-граф, ставит шагу `outcome`.
- `--unclear` → `outcome=fail(needs_clarification)`: gate пропускается правилом 3. Сегодня финишер сам `--force`-закрывает gate
  (`commands/planner/submit/run.py:96-99`) — уходит.
- **Не хватает:** (а) gate — шаг, который никто не исполняет и который закрывает внешнее решение (`<cli> approve|return <run>`), с
  таймаутом и причиной. В 5 правилах его нет. (б) `when` — условие при инстанцировании (тривиально, но тоже не в списке).
- Gate бидом делать не стоит: `br ready` отдаёт бид типа `gate` как готовый **[П §6]**, а `br gate` — другое понятие (отчёт 11).

### 3.3 plan-review / plan-crossreview

```toml
[[steps]] id="round"  loop={ max="{{rounds}}", until="all(lanes.verdict == 'approve')", on_exhausted="review_rounds_exhausted" }
  [[steps.children]] id="plan"          agent="planner"              session="fresh"
  [[steps.children]] id="review"        agent="plan-reviewer"        needs=["plan"] lane="claude"
  [[steps.children]] id="review-codex"  agent="plan-reviewer-codex"  needs=["plan"] lane="codex"   # только crossreview
[[steps]] id="approve"     type="gate" by="human" needs=["round"] when="approval == human"
[[steps]] id="materialize" exec="sv planner materialize" needs=["round","approve"]
```

- Правила 1, 3, 4. Две lane готовы одновременно → две параллельные сессии (claude + codex).
- **Правило 4 надо расширить.** В 07 это «тот же бид переоткрывается». Здесь тело цикла — группа из 2–3 шагов разных ролей,
  каждый раунд — новые fresh-сессии, номер раунда виден финишеру (сегодня `gc.attempt`), вердикт lane — результат шага *этого*
  раунда. Нужно: цикл над группой, результат шага как KV, `until` над результатами членов, `max` из переменной.
- Что уходит из пака: `check.py` (49 строк) → выражение; «last lane decides» и ручной hard-fail на раунде `rounds`
  (`commands/plan-reviewer/verdict/run.py:115-131`) — сегодня это обход того, что `max_attempts` только литерал
  (`SV/formulas/servitors.plan-review.toml:27-31`); гонка двух lane на создание бидов (`assets/planner/plan.py:254-289`).
- Планировщик раунда 2 — fresh-сессия, читает комментарии `Review r1 …` на задаче. `continuation_group` не нужен.
- Бидов: сегодня 9–14 + конвой; B: 3–4 (переоткрываются); C: 0 до одобрения, потом N ≤ 5 task-бидов.

### 3.4 implement (+ implement-task)

```toml
[[steps]] id="build"  for_each="children(target, status=open)"  agent="implementer"  session="fresh"
          max_parallel=3  exclusive=true  on_item_fail="stop_new"  pre="sv implementer start"
```

- Правила 1, 3, 5. Item-формула из одного шага вырождается: **ребёнок F сам и есть шаг**. Сегодня на задачу четыре сущности
  (task-бид, unit-конвой, item-root, step-бид), и `done` закрывает сначала шаг, потом задачу
  (`commands/implementer/done/run.py:165-169`). Под-эпик нужен только многошаговым item-формулам.
- Волны — `br ready --parent F` после каждого закрытия; порядок — `blocks` из плана.
- `exclusive` = реестр прогонов в state по (formula, target): повторный запуск возвращает живой прогон. Утечка
  `gc.exclusive_drain_reservation` (`SV/skills/implementer/SKILL.md`, «Recover») исчезает.
- **Не хватает (1): исход `requeue`.** `start` видит, что файлы заняты более ранней задачей, добавляет ребро `blocks` и отпускает бид
  (`commands/implementer/start/run.py:98-106`). Нужен `<cli> step release --blocked-by <bead>` — вернуть бид в ready-очередь
  без провала и без счётчика попыток.
- **Не хватает (2): pre-dispatch exec.** Сегодня сессия стартует в каталоге роли и узнаёт worktree только после claim: claim —
  pull (`start/run.py:4-7`). При push оркестратор знает бид до старта: `pre` зовёт скрипт пака (логика spine/sub-task остаётся в
  `task.py`), тот возвращает `{cwd, context}` | `{skip}` | `{requeue}`. Сессия стартует сразу в worktree.
- **Развилка «набор ≠ эпик».** Для плана и QA набор = открытые дети F (устаревшие `materialize` закрывает `abandoned`) — совпадает
  с сегодняшним конвоем. Случай B из skill («свои биды, без плана → `gc convoy create`») превращается в «создай эпик»; бид с чужим
  parent туда не положить → селектор по label `set:<id>`, v1.

### 3.5 qa

```toml
[[steps]] id="round"  loop={ max=5, until="fix.skipped", on_exhausted="qa_rounds_exhausted" }
  [[steps.children]] id="run"  exec="sv tester run"  timeout="{{test_timeout}}"
  [[steps.children]] id="review-code" agent="qa-reviewer-code" needs=["run"] when="qa_code" skip_if="kv(target,'qa.code')"
  # … test, docs, security, dependency — так же
  [[steps.children]] id="fix"  run="servitors.implement"  on="target"  select="label:qa-fix"  needs=["run","review-*"]
                     skip_if="count(select) == 0"  timeout="{{fix_wait}}"
```

- Правила 1, 2, 3, 4, 5.
- `run`: exec вместо фальшивого агента. Уходят claim-цикл и heartbeat-строки, которыми скрипт держит панель «живой»
  (`assets/tester/session.py:26-38`, `common.py:14`).
- `gather` сегодня: `convoy create` → `gc sling` → `sleep 30`-опрос до 2 часов (`assets/tester/qa.py:205-251`).
- **Не хватает (1): шаг-«дочерний прогон»** (`run=`): ждёт завершения по событию, наследует fail, имеет таймаут. Это обобщение
  правила 5 (под-эпик без for_each), но отдельная конструкция.
- **Не хватает (2): пропуск соседнего шага из exec** — lane, уже прошедшая для F, закрывается `skipped` без сессии
  (`qa.py:150-154`). Либо `skip_if` над KV бида, либо `<cli> step skip <id>`.
- Fix-биды — настоящая работа, остаются бидами br (дети F, label `qa-fix`, `lane:<x>`).
- Бидов: сегодня ~20 шаговых на раунд + fix-прогон; C: только fix-биды (0–7).

### 3.6 Чего не хватает пяти правилам — сводка и проверка на `metropolis.rig-task`

| № | Нужно | Кому | Правило |
|---|---|---|---|
| 6 | gate (human; условие/таймер — позже) с таймаутом и причиной | plan, plan-review; 4 gate в rig-task (`blocking-rigs`, `human`, `epic-qa`, `land`) | новое |
| 4′ | цикл над группой, `max` из var, результат шага как KV, `until` над членами | plan-review, qa; 3 check-петли rig-task | расширение 4 |
| 5′ | шаг = дочерний прогон с ожиданием и таймаутом | qa.fix; shared drain rig-task с многошаговой item-формулой | расширение 5 |
| 7 | исход `requeue` / release | implement | новое |
| 8 | `when` при инстанцировании, `skip_if` в рантайме | plan, qa; 29 `condition` у владельца | новое, тривиальное |
| — | wait/wake сессии | scout | примитив сессии |
| — | pre-dispatch exec, выбор cwd | implement | свойство правила 1 |

`metropolis.rig-task` (646 строк, 43–81 бид на прогон) **[П]**:

- scope + `gc.on_fail=abort_scope` на каждом шаге + `teardown` (`gc.kind=cleanup`) = правило 3 + шаг `always`.
- `expand = "metropolis.quality-loop"` ×2 = включение шаблона при компиляции (макрос) — v1, либо копипаста.
- shared single-lane drain с item-формулой `implement-group` = правило 5 с настоящими под-эпиками и `max_parallel=1`.
- **`gc.continuation_group="plan"`** на трёх шагах planner (строки 265, 312, 411) = `session="continue:plan"`: оркестратор
  хранит id сессии в state прогона и подаёт следующий шаг новым ходом в ту же сессию; между ходами процесс можно остановить и
  сделать resume. Это многоходовая беседа, а не «удержание панели». Механизм тот же, что у wait/wake (§3.1): он появится в v0,
  атрибут — в v1.

Вывод: всё выразимо; полный перенос не нужен.

## 4. Сквозной сценарий v0

Пайплайн на риге `gascity-packs`: задача `T` → **plan-review** (`approval=human`, по ходу planner спрашивает scout) → gate →
materialize (3 задачи) → **implement** (fan-out) → **qa** (раунд 1: 2 fix-бида; раунд 2: pass). Вариант C. Всё ниже — **[В]**.

| # | Событие | br | state | Сессии, хуки, MCP | Оператор |
|---|---|---|---|---|---|
| 1 | `<cli> run servitors.plan-review --on T --var approval=human` | — | `run#1`, 5 `run_step` | — | `<cli> runs` |
| 2 | шаг `plan` ready | `update T --claim --actor s1` | step→running, session s1 | старт claude opus (fresh), cwd — каталог роли; настройки профиля + deny readonly + `deny-unbounded`; MCP `kb` (URL от реестра), `deja` (stdio); промпт = роль + бриф + T | `<cli> sessions`, `tail s1` |
| 3 | planner: `sv scout ask … --for T`, `sv scout wait` | comment на T | `run#2` (explore); wait(s1 ← run#2) | конец хода s1; старт scout (haiku) | — |
| 4 | scout: `sv scout answer` | comment на T | run#2 pass → wake s1 | новый ход в s1: «ответы на T» | — |
| 5 | planner: `sv planner submit` | `update T --design`, comment (граф) | step `plan` pass | s1 закрыта | — |
| 6 | lane `review` ready | — | step running | старт reviewer (opus, fresh) | — |
| 7 | `verdict iterate` | comment `Review r1` | result verdict=iterate; `until` ложно → round 2, шаги сброшены | — | — |
| 8–9 | раунд 2: plan, review → approve | 2 comments, design | round pass | 2 сессии | — |
| 10 | gate `approve` | comment «awaiting approval» | step waiting | уведомление в inbox (dedup по run) | **`<cli> gates`; `<cli> approve run#1`** |
| 11 | exec `materialize` | 3× `create --parent T` (draft) + deps + open; comment | KV `T.plan_digest`; run#1 pass | — | — |
| 12 | `<cli> run servitors.implement --on T` | — | `run#3`, exclusive по T | — | — |
| 13 | T1, T2 ready (T3 `blocks` T1) | claim ×2 | 2 шага running | `pre`: `sv implementer start` → worktree, cwd; 2 сессии sonnet прямо в worktree; хук `stop-gate` | `<cli> run show run#3` |
| 14 | `sv implementer done` ×2 | close ×2, comments «built by» | KV `integrated`, `commits` | git rebase/ff в скрипте пака | — |
| 15 | T3 ready → сессия → done | close, comment | run#3 pass | — | уведомление «run#3 pass» |
| 16 | `<cli> run servitors.qa --on T` | — | `run#4` | — | — |
| 17 | exec `run`: format, lint, tests; tests красные | commit autofix; create fix-бид; comment | KV `qa.fixes` | — | — |
| 18 | 5 lane параллельно | comments; 1 fix-бид от lane code | 4 pass → KV `qa.<lane>` на T | 5 сессий (4 opus, 1 sonnet) | — |
| 19 | шаг `fix` = дочерний `servitors.implement` по `qa-fix` | dep add; 2 claim/close | `run#5` | 2 сессии implementer | — |
| 20 | раунд 2: `run` зелёный; lane code → pass; остальные 4 `skip_if` | comment «QA passed» | run#4 pass | 1 сессия | уведомление |

Итог: 5 прогонов, 17 сессий LLM, ~20 хопов, 1 подтверждение человека.

| Метрика | Сегодня | Новая система |
|---|---|---|
| Хоп «шаг закрыт → следующий агент работает» | 22–39 с (`MP/README.md:2406-2431`, по отчёту 02); 19,0 с в измеренной цепочке (отчёт 05) | exec ~0,1–0,3 с; LLM-шаг 1–3 с (операция br 30–150 мс на малой БД — отчёт 11; старт процесса claude **[НП]**) |
| Чистая задержка на 20 хопов | 7–13 мин | 0,5–1 мин |
| Бидов в трекере на сценарий | ~85 шаговых и control + 6–8 конвоев + 5 рабочих **[В]** по `LAB/runs.py` | **5 рабочих** (3 задачи + 2 fix) |
| Комментариев / прочих записей br | ~20 / ~15 | те же ~20 / ~15 |
| Строк state | 17 session-бидов × 60–83 metadata-ключа (отчёт 06) | 5 `run` + ~40 `run_step` + 17 `session` + ~15 KV |
| Фон за сутки простоя | ~5 000 order-wisps, ~40 SELECT/с (отчёт 06) | 0 |

## 5. Минимум v0

### 5.1 Что система обязана уметь

1. Трекер за адаптером (~10 операций): явный `BEADS_DIR`, уникальный `--actor` на сессию, повтор при отказе claim.
2. **KV-аннотации бида и результат шага** в state + CLI для скриптов пака.
3. Движок: правила 1–5 с расширениями 4′ и 5′, gate (human), `requeue`, `when` / `skip_if`.
4. Реестр прогонов: (formula, target) → run; exclusive; `run show`.
5. Роли: харнесс, модель, effort, env, права, pool-лимит, fresh; named resume-сессия `mayor`.
6. Драйверы claude и codex. **Модель на сессию обязательна** (в паке три модели Claude); по сводке, у claude-agent-acp она
   ставится только через `session/set_config_option` — проверить на трёх ролях сразу **[НП]**.
7. Push-доставка: промпт = шаблон роли + фрагменты + бриф + вход; env `BEAD_ID`, `RUN_ID`, `RIG`, `RIG_ROOT`; `bin` пака в PATH.
8. Pre-dispatch exec с выбором cwd.
9. Wait/wake сессии по закрытию бидов.
10. Хуки: `[[hook]]` в диалекте Claude → claude и codex (`bypass_hook_trust`); deny-правила на роль.
11. MCP: декларация + сервис под супервизором (kb) + stdio на сессию (deja); отсечение личного конфига оператора.
12. Liveness по событиям хода: тишина → interrupt → новая сессия ≤ N → defer + уведомление; «ждёт лимита» ≠ «завис».
13. Уведомления оператору с dedup; `<cli> sessions | tail | send | gates | approve | runs`.
14. Rig: путь, default branch, `formula_vars`.

Сверка с 14 must отчёта 02 §4:

| Must | v0? | Комментарий |
|---|---|---|
| 1. Риги; эпик над бидами нескольких ригов | частично | межриговый эпик servitors не использует → v1. Но «одна `.beads` на город, риг = label» решать **сейчас** — это раскладка данных |
| 2. Стор, включая metadata KV и атомарный граф | да | KV — в state; граф — draft → open |
| 3. Роли, fresh/resume, pool | да | resume только для mayor |
| 4. Скриптовые агенты | **заменено** | exec-шаги; сущность не нужна |
| 5. Claim-протокол | да, проще | push; «один claim на разговор» получается сам |
| 6. Маршрут, deps, условие, петля, fan-out, scope + cleanup | да, кроме cleanup | шагов cleanup/`always` в servitors нет → v1 |
| 7. Гейты human + внешнее условие | human — да | таймер и условие → v1 (нужны только metropolis) |
| 8. Worktree, сессия стартует в нём | да | через п. 8 выше; подсистема worktree в ядре не нужна |
| 9. Pack-расширения | да | команды = файлы в PATH; шаблоны, фрагменты, vars |
| 10. Хуки на роль; prime / handoff / mail-инъекция | да / v1 | prime = промпт оркестратора; handoff при компакции нужен только mayor |
| 11. Общий MCP, opt-out по роли | да / v1 | opt-out и «правка MCP не рестартит» → v1 |
| 12. Liveness | да | заменяет stall-sweep |
| 13. Событийный хоп | да | ядро |
| 14. Сессия оператора и почта | облегчённо | inbox + CLI; `takeover` / ACP-шим → v1 |

Из «should» в v0 нужен только сервис пака. **Ордера, doctor-проверки, usage, `config explain`** — v1: без двух sweep-ов своих
ордеров у servitors нет, индексатор kb тикает внутри сервиса.

### 5.2 Что меняет пак

1. Тонкий `lib/tracker.py` вместо `gc("bd", …)`: `show / comment / update / close / list / kv / step_done` (~154 места).
2. Запросы `--metadata-field gc.*`, чтение `gc.outcome / gc.attempt / gc.root_bead_id` → `<cli> current --json`.
3. Формулы — заново (~150 строк); `materialize`, `tester run/gather` — exec-шаги.
4. Убрать: оба ордера, `session.py`, `check.py` ×2, код конвоев, `scout wait`, симлинк-оверлеи, провайдеры, `pre_start`.
5. Идемпотентность `materialize`: вместо детерминированных ID — KV `plan_digest` на T + поиск детей по label.
6. Хук `replan` → exec-подготовка входа перед LLM-шагом (правило «Deterministic input», `RULES:147`).
7. Промпты: убрать шаг «Claim»; `gc servitors X` → `sv X`; `gc bd show/comments` → `br show/comments`; skills мэра про
   `gc sling` / `gc convoy` / `gc session wait` — по смыслу.
8. Deny-правила и codex-rules: `br update/close`.
9. Определить `mayor`; `desk` упразднить.
10. Тесты: `fakegc.py` → фейк адаптера.

## 6. Данные

Снимок `lab/06-storage/issues_*.json` (10:10; только `id, issue_type, metadata, status`), классификация `LAB/snap.py` **[П]**:

| Стор | Всего durable | Граф (шаги, control) | Конвои синт. / наст. | Работа | Не закрыто |
|---|---|---|---|---|---|
| hq (`gc-`) | 339 | 241 | 12 / 4 | 82 | 2 |
| mp-api (`mpa-`) | 415 | 391 | 3 / 7 | 14 | 0 |
| mp-front (`mpf-`) | 112 | 96 | 3 / 2 | 11 | 0 |
| gascity-packs (`gcp-`) | 98 | 66 | 5 / 1 | 26 | 14 |
| dotfiles (`dot-`) | 4 | 3 | 0 / 0 | 1 | 1 |
| **Итого** | 968 | 797 | 23 / 14 | **134** | **17** |

Мой классификатор строже отчёта 06 (~280): бид с `gc.root_bead_id` считается графом. Плюс 34 483 wisps в hq — не переносятся.

Эксперименты `LAB/imp*`, br 0.7.6 и 0.6.0 дали одинаковый результат **[П]**:

| Вопрос | Результат |
|---|---|
| Сырой `bd export` → `br sync --import-only` | **отказ**: `Invalid JSON … invalid type: string "1", expected i64` — id комментария у bd строка (образец `lab/br/compat/bd-export.jsonl`) |
| `metadata` в строке JSONL | импорт проходит; в БД поля нет (`br show --json` без `metadata`); после первой мутации исчезает и из JSONL — **тихая потеря** |
| Metadata в labels | `servitors.branch=servitors/gcp-bbb` → отказ: «only alphanumeric, hyphen, underscore, colon allowed». Путь, ветку, sha в label не положить |
| ID `gcp-bbb.r1ab12.1` (формат ID планировщика) | отказ: «id: invalid format (expected prefix-hash)»; `gcp-bbb.1` проходит |
| Разные префиксы (`gc-`, `mpa-`, `gcp-`) в одной БД, parent и `blocks` между ними | проходит; `br ready --epic gc-epic1` вернул `mpa-aaa` |
| Новые биды с префиксом рига | у `br create` флага префикса нет (только `--slug`) → **все новые биды получат один префикс города** |
| Custom-типы `spec`, `gate`, `convoy` | импортируются, но **все попадают в `br ready`** как готовые |
| Ребро `tracks`, статус `deferred`, `design`, `acceptance_criteria`, comments | сохраняются |
| Claim | `--actor s1` → ok; `--actor s2` → `VALIDATION_FAILED: already assigned to s1` |

Рекомендуемая процедура **[В]**:

1. **Историю не переносить.** `bd export` всех 5 сторов → JSONL-архив в git. Dev-режим города разрешает терять историю
   (`CITY/AGENTS.md:22-28`).
2. **Переносить незакрытые рабочие биды (17) и их открытых предков** трансформером: comment `id` → int; `metadata` разобрать;
   ID `<task>.<seg>.<n>` → `<task>.<n>` (старый — в comment); типы `spec` / `gate` / `convoy` / `step` не переносить.
3. **Одна `.beads` на город** (отчёт 11), вне checkout-ов ригов; rig → label `rig:<name>`. Старые ID сохраняются, новые получат
   общий префикс: соглашение «префикс = риг» теряется.
4. **Metadata:** `servitors.*` на F (`branch`, `worktree`, `qa.<lane>`, `stalls`, `integrated`, `commits`) → KV в state;
   `gc.work_outcome` → `close_reason`; остальные `gc.*` и `metropolis.*` (38 бидов, все закрыты) — только в архив.
5. **Настоящие конвои (14)** → эпик либо label `set:<id>`; **gates** (12 в истории) не переносить: открытые прогоны довести в
   старом городе.
6. `.beads/*` у владельца в глобальном gitignore (отчёт 11 §2.2) — бэкап JSONL коммитить явно.

**Параллельная жизнь** возможна: Dolt и br не пересекаются. Условия **[В]**:

- бид живёт ровно в одном трекере;
- новый город — отдельный каталог; его `.beads` не внутри рига: в ригах лежит `.beads` от bd (`CITY/.beads/routes.jsonl`), а br ищет
  `.beads` вверх по дереву (инцидент отчёта 11) → `BEADS_DIR` обязателен в каждой сессии и в каждом exec;
- ветки `servitors/<F>` и worktree ключуются id бида → не столкнутся, пока биды разные;
- kb в фазе 0 — существующий сервис старого города по прямому URL (второй `bm` = +296 МБ);
- общий лимит одновременных сессий на оба города: квота подписки одна.

Полигон: `dotfiles` годится только для дымового explore (1 бид, lint/test пустые — `CITY/city.toml:143-153`). Настоящий полигон —
`gascity-packs` (тесты пака идут секунды, 14 открытых бидов).

## 7. План по фазам

| Фаза | Содержание | Критерии выхода | Откат |
|---|---|---|---|
| **0. Полигон** | Новый каталог города; риг `gascity-packs`; новый бид в br (без миграции); формулы `explore`, затем `plan` с `approval=human`. Проверяются: push, модель на роль, хуки и deny, kb + deja, финишер, KV, gate, уведомление, liveness | 10 explore и 5 plan подряд без ручного вмешательства; хоп p95 ≤ 5 с; 0 записей вне `BEADS_DIR`; kill -9 сессии и демона посреди шага → шаг доработан или перезапущен без дублей; личные хуки оператора не срабатывают | удалить каталог; старый город не затронут |
| **1. servitors целиком** | `plan-review`, `crossreview` (codex), `implement`, `qa`; адаптер в паке; новый `mayor`; перенос 17 открытых бидов; старый город в `gc suspend` для перенесённых ригов | сценарий §4 на реальной задаче 3 раза; тесты пака зелёные на фейке адаптера; implementer ×3 параллельно без потерь слияния; QA в 2 раунда; неделя без sweep-ордеров и без зависания, не замеченного liveness | `gc resume`; биды вернуть обратным трансформером (потеряются только KV) либо доработать руками — город в dev-режиме |
| **2. Вывод из эксплуатации** | Экспорт-архив Dolt; остановка `gc-supervisor` (pitchfork), `dolt sql-server`, `tmux -L gc`; удаление импортов `bd` / `core` / `metropolis`, 8 order-overrides, `.dolt-backup` (1,4 ГБ); правка dotfiles | 2 недели фазы 1 без обращения к старому городу; архив читается; RSS оркестратора + сервисов < 400 МБ в простое против ~1,9 ГБ сегодня (dolt 0,8–1,4 ГБ + gc 0,24–0,37 ГБ + kb 0,33 ГБ) **[В]** | архив + пин версий в dotfiles: старый город поднимается из git |

## 8. Риски и неизвестные (топ-10)

| # | Риск | Оценка, смягчение |
|---|---|---|
| 1 | **Нет metadata в br; пак построен на ней** (20 своих ключей, 9 запросов по ключу) | KV в state — обязательное условие, не опция. Без него мигрировать нечего |
| 2 | **Тексты агентов привязаны к `gc` / `bd`**: 124 упоминания, 25 файлов; ещё ~15 f-строк в скриптах печатают оператору `gc sling …` | compat-шим `bd → br` **не нужен**: читающих команд ~31, а claim / sling / wait / convoy меняют смысл, а не имя. Достаточно `sv` + прямого `br show/comments` |
| 3 | **fresh vs resume.** Все роли servitors fresh — совпадает с push-моделью. Resume нужен mayor и «wake после scout wait» | wake требует resume остановленной сессии; у claude после resume модель сбрасывается (сводка) → переустанавливать при каждом wake. **[НП]** |
| 4 | **Жизненный цикл worktree** остаётся в паке (spine / sub-task, parked-ветки, `reset`) — ~700 строк git-логики; provenance-проверка `gc worktree verify` теряется | приемлемо: пак и так зовёт git напрямую. Гонка двух `done` на feature-worktree остаётся (5 попыток, `done/run.py:136-144`) |
| 5 | **Human gate** сейчас — бид + письмо + команда мэра. В state он не виден в bv | `<cli> gates` + комментарий на задаче. Таймаут gate — новое поведение; дефолт согласовать |
| 6 | **Хуки codex и доверие.** Доставка override-ом треда + `bypass_hook_trust` сквозным путём не прослежена (отчёт 18 §5); sandbox codex в контейнере не работает → read-only = политика; `apply_patch` не закрыт (бид `gcp-9ye`) | в фазе 0 прогнать `deny-unbounded` в codex-сессии до crossreview. **[НП]** |
| 7 | **Dry-run.** Риги `mp-*` — `dry_run="true"`, scratch-origin (`CITY/city.toml:114-135`); publish / PR-блока в servitors ещё нет | v0 наружу не пишет вовсе — безопасно, но «боевой» путь (push, PR, merge) этим переездом не проверяется |
| 8 | **br**: движок моложе года, один мейнтейнер, «refuse instead of lose», 25 релизов за 10 недель, rider в лицензии (отчёт 11) | pin; адаптер ≤ 10 операций; запасной выход — свой SQLite-стор. Владелец пинит 0.6.0 — она вдвое-втрое медленнее 0.7.6 |
| 9 | **Потеря знаний о гонках**: `RULES` §8 — ~580 строк проверенного поведения gc | перенести как тест-кейсы движка: «зависимый шаг стартует после провала предшественника», «две lane сразу», «release при busy files» |
| 10 | **Конфиг оператора протекает в сессии** (язык, плагины, `trauma_guard`): `RULES`, «The operator's own settings reach every gc session» | `settingSources=[]`; но плагин deja ставится через `enabledPlugins` в settings — как включить его при отсечённом user-scope, не проверено **[НП]** |

Прочие неизвестные: один DB-префикс против привычки владельца к `mpa-` / `gcp-`; `skillOverrides` и `enabledPlugins` из
`base/settings.json` (18 выключенных skills, 5 плагинов) через per-session settings **[НП]**; `stop-gate` при cwd = worktree с
самого старта (хук отличает worktree по «ниже каталога роли»).

## 9. Вывод

**Достаточно ли «минимального ядра»?** Да, если «минимальное» — не «эпик + дети», а 14 пунктов §5.1: push-диспетчер, 5 правил
с двумя расширениями, gate, `requeue`, wait/wake, KV в state, per-session конфиг и хуки, сервис + MCP-реестр, liveness, inbox.
На них servitors переезжает целиком, теряя ~18 % Python и все свои ордера; metropolis выразим, но не нужен. Город — dev-полигон:
переносить 17 бидов, откат дёшев на любой фазе.

**Оказалось нужнее, чем предполагал владелец:**

1. **KV-состояние рядом с бидом** — «просто br» не хватает на первом же финишере.
2. **Граф с циклом над группой, gate и вложенным прогоном** — «молекулы лишние» верно для сущности, неверно для способности: на
   цикле стоят plan-review и qa.
3. **Exec-шаги первого класса**: tester, materialize, подготовка входа, pre-dispatch — детерминированная половина пайплайна.
4. **Wait/wake и удержание сессии между ходами** — здесь ACP действительно лучше tmux, и это нужно уже в v0.
5. **Узкий стабильный CLI для скриптов пака** (~12 операций): пак — программа на 154 вызова, а не набор промптов.

**Не нужно вовсе (для этого города):**

1. **Конвои** — все 10 вызовов и ордер-подметальщик существуют ради движка gc.
2. **Ордера в v0** — у servitors их остаётся ноль; из 10 записей про ордера в `city.toml` все про Dolt и гигиену.
3. **Подсистема worktree, pull-claim (`hook --claim`, `drain-ack`), script-агенты как сессии** — заменяются выбором cwd, push и exec.
4. **Почта как подсистема** — 4 вызова, 12 писем за 22 дня: уведомления с dedup.
5. **Материализация overlay / skills / settings в workdir, провайдеры-носители, ответы на диалоги** — исчезают вместе с TUI.
6. В v0 также не нужны pi, межриговый эпик, cleanup-шаги, gate-таймеры, общий ACP-сервер ради памяти (сессии fresh, живут
   десятки секунд).

### Вердикты по гипотезам (вклад этого отчёта)

- **H5 — частично.** br как трекер работы проходит проверку данными владельца (смешанные префиксы, parent, `blocks`, comments,
  claim). Как носитель состояния пака — нет: metadata теряется молча, labels не вмещают значения.
- **H6 — частично.** Конвои и step-биды уходят без потерь; граф остаётся и требует на 4 конструкции больше «5 правил».
- **H8 — подтверждается для сценария:** 7–13 мин ожидания на 20 хопов → меньше минуты; обе тиковые подметалки исчезают.
- **H9 — по сути подтверждается:** провайдер-носитель, overlay-симлинки и `-c hooks.*` сводятся к одному объявлению.
- **H11 — поддерживается:** цена для пака — ~620 строк в минус, ~154 механические правки, формулы заново.

### Не проверено

- Ни одна сессия харнесса не запускалась: задержки хопа — оценка по компонентам из отчётов 05, 11, 14.
- Снимок бидов — от 10:10, без parent / title / deps: число «открытых предков» для переноса не посчитано.
- Доля тестов пака, завязанных на конвои и `gc.*` (~40 %), — по именам файлов. Строки «исчезает / остаётся» — чтением кода.
- `docs/*.md` пака, `PACKS/tmp/kb-*.md` и аудит metropolis просмотрены выборочно (аудит — через отчёт 02).
- Доставка `skillOverrides`, `enabledPlugins`, deny-правил через per-session settings и хуков через override треда codex — по
  отчётам 14, 15, 18. Обратный трансформер br → bd для отката не писался.

# 02 — Что из Gas City реально используется в городе владельца (H7, H6, H9, H11)

Взгляд со стороны пользователя: конфиги, паки, логи, runtime-состояние, процессы. Снято 2026-11-10 ~10:10 (+07).
Ничего не менялось; запускались только `ps`, чтение файлов, `gc --help`, `gc config explain`, `gc mcp list`,
`pitchfork logs`, `deja`.

Сокращения путей: `CITY` = `/home/ubuntu/proj/share/gc`, `PACKS` = `/home/ubuntu/proj/work/gascity-packs`,
`MP` = `PACKS/packs/metropolis`, `SV` = `PACKS/.claude/worktrees/servitors-kb/packs/servitors`,
`AUDIT` = `PACKS/docs/metropolis-audit-2026-10-29.md`, `RULES` = `PACKS/docs/new-pack-rules.md`.

## 1. Выводы за 60 секунд

- Город — **полигон для разработки пака**, а не продакшен: за 22 дня (19.10–10.11) 48 запусков формул, 199 начатых
  шагов, 3 эпика с dry-run PR (`AUDIT:39-51`, `CITY/AGENTS.md:18`). Выводы о «нужном» сделаны на малой выборке.
- Используется узкая полоса gc: **10 из 34** секций конфига города, **16 из 57** полей агента, **~24 из 61**
  встроенной команды CLI, **1 из 6+** session-провайдеров (tmux), **2 из 3** критичных харнессов (claude, codex;
  `pi` объявлен, запусков 0), ACP — 0 запусков, k8s/extmsg/webhooks/github/converge/hosted login — 0.
- При этом ядро **formula v2 используется плотно**: `needs`, `condition`, `[steps.check]` (retry-петли),
  `[steps.drain]`, `[steps.gate]`, `expand`, scope/`abort_scope`, cleanup, `continuation_group`. «Выкинуть формулы»
  нельзя; можно выкинуть их вес.
- Чтобы жить на gc, владелец написал **26 тыс. строк Python** клея в `metropolis` (+35 тыс. строк тестов, 302
  коммита) и сам же признал четверть лишней, а пак — тупиком («Metropolis is not going to be fixed», `AUDIT:3`).
  Новый пак `servitors` — 3.6 тыс. строк с жёсткими бюджетами.
- **91 % созданных бидов — служебные** order-tracking виспы (9 841 из 10 795); **77 % событий** — `order.fired/completed`
  (195 493 из 252 445). Инфраструктура шумит на два порядка громче работы.
- **Главный потребитель LLM в городе — обслуживание Dolt**: агент `bd.dog` из встроенного пака `bd` — 392 из 1 177
  записей usage, 130 сессий, 534 тыс. output-токенов (у worker'а 140 тыс.). 1–9 ноября город простаивал, и все
  54 пробуждения сессий — `bd.dog-1`.
- Dolt сейчас: 0.8–1.4 ГБ RSS + 292 МБ swap, **26 % одного ядра в среднем** за 14 суток. tmux — 2 МБ. Память
  съедают не tmux, а Dolt и процессы харнессов (claude 212–259 МБ на сессию).
- Конвои владелец **как фичу не использует ни разу**: все 24 конвоя синтетические (`input convoy for …`,
  `input of …`). Цена — ~450 строк шима, отдельный sweep-ордер, обход бага `gc convoy add`.
- H9 в формулировке владельца **неточна**: проекция MCP из пака на всех агентов, слоёные провайдеры и
  `[[service]]`-прокси в gc есть, и владелец уже поднял через них общий на город HTTP-MCP (`kb`). Нет другого:
  шаринга stdio-MCP, стандартного слоя хуков для авторов паков, наслоения overlay, отказа от MCP по агенту.
- Миграция herdr→tmux (2026-10-26 01:46) случилась из-за того, что herdr не отвечал на стартовые диалоги
  харнессов; tmux отвечает на них **скрейпингом панели и нажатием клавиш**.
- Самое болезненное в эксплуатации — не квоты/429 (живых инцидентов в логах нет), а: тик 30 с, сессии «busy
  forever», диалоги доверия/логина, осиротевшие запуски, правила загрузки настроек Claude/Codex.
- Минимальный обязательный набор для переезда — 14 требований (§5). Это заметно меньше gc, но в нём остаются
  движок графа шагов и жизненный цикл сессий — самые нетривиальные части.

## 2. Факты

### 2.1 Версии и состав

| Что | Значение | Источник |
|---|---|---|
| gc в городе | `1.5.1-edge.a86d05bd7`, собран 2026-10-27 (не HEAD `f1f75f53c`) | `command gc version --long` **[ПРОВЕРЕНО]** |
| Импорты | `bd`, `core` (встроенные, пин по sha), `metropolis`, `servitors` (локальные пути) | `CITY/pack.toml:14-29` **[ПРОВЕРЕНО]** |
| Named sessions | `mayor` (sonnet) и `desk` (opus), обе `mode="always"` | `CITY/pack.toml:34-42` **[ПРОВЕРЕНО]** |
| Риги | 4: `mp-api`, `mp-front`, `gascity-packs`, `dotfiles`; лежат вне города | `CITY/city.toml:114-153`, `CITY/AGENTS.md:62` **[ПРОВЕРЕНО]** |
| Session provider | `tmux`, socket `gc`; reconciler `legacy` | `CITY/city.toml:103-112` **[ПРОВЕРЕНО]** |
| Агентов после резолва | 120 (роли × 4 рига + неявные агенты провайдеров `claude`/`codex`/`pi`) | `gc config explain`, `grep -c '^Agent:'` → 120 **[ПРОВЕРЕНО]** |
| Upstream-паки (gastown, gascity) | не импортируются | `CITY/pack.toml` **[ПРОВЕРЕНО]** |

Роли `metropolis` (13): 11 LLM + 2 скриптовых. Харнесс и модель по `MP/agents/*/agent.toml` **[ПРОВЕРЕНО]**:

| Роль | Харнесс / модель / effort | Pool | wake |
|---|---|---|---|
| desk | claude opus / medium | named | resume |
| mayor | claude sonnet / medium | named | resume |
| planner, plan-reviewer, reviewer, qa | claude opus / high | 8, 2, 2, 2 | resume (planner), fresh |
| worker | claude sonnet / high | 3 | resume |
| tester, answerer | claude sonnet / medium | 2 | fresh |
| group-reviewer | claude haiku / medium | 2 | fresh |
| janitor | codex gpt-6-luna / high (провайдер `metropolis-codex`) | 2 | fresh |
| runner, city-runner | `start_command` (sh→python), `lifecycle="one_shot"`, без LLM | 2 | — |

`servitors` (11): scout (haiku), planner/plan-reviewer (opus), implementer (sonnet), 5 QA-ревьюеров,
plan-reviewer-codex (gpt-5.6-terra), tester (скрипт). Все LLM-роли `wake_mode="fresh"`, свой `work_dir`,
`pre_start = git init`, `idle_timeout` **[ПРОВЕРЕНО]** (`SV/agents/*/agent.toml`).

Фактические запуски по событиям `worker.operation` (`start_resolved`, succeeded) в `CITY/.gc/events.jsonl`
**[ПРОВЕРЕНО]**: claude 284, `sh` 143, servitors-claude 17, codex 11, servitors-codex 3, **pi 0, ACP 0**.
Usage (`CITY/.gc/usage.jsonl`, 1 177 записей): sonnet 655, opus 389, haiku 72, codex 16.

### 2.2 Формулы, ордера, хуки

**Формулы — все v2** (`formula_compiler = ">=2.0.0"`), v1-молекулы использует только встроенный `bd` (`mol-dog-*`).

| Конструкция | Где у владельца | Статус |
|---|---|---|
| `needs`, `condition` (одно сравнение), `description_file`, `[vars]` с enum/pattern | все 13 формул | используется |
| `"gc.run_target"` — маршрут шага на роль | 51 вхождение | используется, ключевое |
| `[steps.check]` + exec-скрипт (петля до N попыток) | plan, plan-review, materialize, lint, test (6 шт.) | используется |
| `[steps.drain]` (fan-out по конвою) | `rig-task.implement` (shared, single_lane), `servitors.implement` (separate, exclusive) | используется |
| `[steps.gate]` / `type="gate"` (human, свои типы) | approve, plan-gate, blocking-rigs, epic-qa, land | используется, но gc не читает тип/таймаут гейта (`AUDIT:122`) |
| `expand` + `type="expansion"` | `quality-loop` ×2 | используется |
| scope / `gc.on_fail=abort_scope` / `gc.kind=cleanup` | все formulas metropolis | используется |
| `gc.continuation_group` | planner (3 шага) | используется |
| `[steps.retry]`, `for_each`/`on_complete`, `extends`, `advice`, `compose`, `loop`, `waits_for` | — | не используется; последние четыре владелец счёл нерабочими (`RULES:139-144`) |

Подсчёт: `grep` по `MP/formulas/*` и `SV/formulas/*` **[ПРОВЕРЕНО]**.

**Ордера владельца — 6, все `trigger="cooldown"` + `exec`** (поллинг скриптом каждые 60 с / 2 мин / 5 мин / 10 мин /
30 мин). Ни одного cron, event или formula-ордера (`MP/orders/*.toml`, `SV/orders/*.toml`) **[ПРОВЕРЕНО]**.
Всего в городе срабатывало 30 разных ордеров, 24 из них — инфраструктура `core`/`bd`.

**Хуки харнессов** **[ПРОВЕРЕНО]**:
- gc ставит свои в `CITY/.gc/settings.json`: `SessionStart → gc prime --hook`, `PreCompact → gc handoff --auto`,
  `UserPromptSubmit → gc nudge drain --inject` + `gc mail check --inject`.
- Пак `metropolis`: 6 событий Claude через `CITY/.claude/settings.json` (пишет `gc metropolis hooks install`), те же
  6 для Codex — шестью `-c hooks.<Event>=…` в `args_append` провайдера (`MP/pack.toml:28-38`), всё ведёт в один
  `dispatcher.py` (722 строки) + реестр `hooks.d/*.toml` по ролям. Лог: 4 760 вызовов диспетчера, скрипт
  запускался в 1 694, блокировок 22 (`CITY/.gc/runtime/metropolis-hooks.log`).
- Пак `servitors`: хуки и `permissions.deny` в overlay роли (`.claude/settings.json` + `settings.local.json`),
  для Codex — `-c hooks.*` и `.codex/rules/` (`SV/README.md:117-214`). Хуков три: `deny-unbounded`, `stop-gate`, `replan`.

**MCP** **[ПРОВЕРЕНО]**: один сервер `kb` из `SV/mcp/kb.template.toml` — HTTP, URL
`http://127.0.0.1:8372/v0/city/${GC_CITY_NAME}/svc/kb/mcp`. gc проецирует его в `.mcp.json` (Claude) и
`.codex/config.toml` (Codex) **каждому агенту города** (`gc mcp list --agent mayor|desk|mp-api/metropolis.worker|…janitor`
дают одну и ту же строку). За URL — `[[service]] kind="proxy_process"`: один процесс `serve.py` + дочерний
`bm mcp` на весь город. Второй MCP, `deja`, — stdio, приходит плагином Claude (и `-c mcp_servers.deja` для Codex)
и запускается под каждой сессией.

### 2.3 Матрица «поверхность gc × использование»

Легенда: ✔ используется; ⚠ вынужденно/через обход; ✘ не используется.

| Поверхность | | Комментарий и источник |
|---|---|---|
| `gc bd` (show/list/create/update/close/comments/dep/label/gate/children, `create --graph`, metadata) | ✔ | самый частый вызов в коде паков: ~110 вызовов `bd …` в Python `servitors` (grep по `SV`) |
| `gc sling … --on <formula> --var` | ✔ | единственный способ запуска работы |
| `gc hook --claim` / `hook current`, `gc runtime drain-ack` | ✔ | протокол claim у всех ролей |
| `gc session` list/peek/wait/close/reset/submit/attach | ✔ | `attach` — вход оператора в desk; `peek` — поллинг панелей sweep'ом |
| `gc worktree ensure/verify/cleanup` | ✔ | servitors; metropolis строил свои команды (`AUDIT:184`) |
| `gc mail` send/inbox/archive, `--notify`, `--dedup` | ✔ | только mayor и `human`; за событиями: `mail.sent` = 12 за 22 дня |
| `gc formula show/catalog/spec`, `gc prime`, `gc config`, `gc rig`, `gc doctor`, `gc import`, `gc reload`, `gc suspend/resume` | ✔ | эксплуатация |
| `gc service`, `gc mcp list`, `[[service]]`, `mcp/` пака | ✔ | с ноября, ради `kb` |
| pack commands (`gc metropolis …` 34 шт., `gc servitors …`), doctor-проверки пака, skills пака, `template-fragments`, `overlay_dir` | ✔ | механизм расширения — основной |
| `gc convoy` create/add/status/close/delete | ⚠ | только чтобы удовлетворить `sling` и `drain` (см. 2.5) |
| `gc wait list/ready`, `gc session wait` | ⚠ | у `wait` нет таймаута → свои timer-гейты (`AUDIT:131`) |
| `gc order` + `[orders] skip/overrides` | ⚠ | 8 переопределений в `city.toml`, чтобы утихомирить встроенные ордера (`CITY/city.toml:28-99`) |
| `gc nudge` (как механизм: поле `nudge`, `nudge-on-route`) | ⚠ | работает набором текста в панель; nudge отвечает на `AskUserQuestion` первым вариантом (gascity#8150, `MP/README.md:1797`) |
| `gc dolt …`, `mol-dog-*`, `dolt-remotes-patrol`, `beads-health` | ⚠ | не нужны владельцу сами по себе — обслуживание хранилища |
| `gc events`, `gc event emit` | ✘/⚠ | emit отвергнут как best-effort (`MP/README.md:2518-2520`); event-ордера не взяты |
| dashboard / HTTP API | ⚠ | API используется как `/health` и прокси `svc/kb`; socat публикует 8372→8373; дашборд в процессе владельца не фигурирует **[НЕ ПРОВЕРЕНО]** как часто открывается |
| `gc handoff` | ✔ (неявно) | только через встроенный `PreCompact`-хук |
| `agent-script`, `analyze`, `build-image`, `context`, `converge`, `costs`, `extmsg`, `github`, `graph`, `login/logout/whoami`, `maintenance`, `metrics`, `pack`, `shell`, `skill`, `storage`, `trace`, `beads`, `lint`, `ready`, `prompt`, `agent` | ✘ | нет в коде паков и в docs владельца; `usage.jsonl` — `"unpriced":true` |
| Конфиг: `[storage]`, `[beads]`, `[ops]`, `[mail]`, `[events]`, `[usage]`, `[dolt]`, `[api]`, `[chat_sessions]`, `[session_sleep]`, `[convergence]`, `[maintenance]`, `[[webhook]]`, `[github]`, `[extmsg]`, `[upstreams]`, `[patches]`, `[pricing]`, `include`, `[defaults]`, `[session.k8s]`, `[session.acp]` | ✘ | `CITY/city.toml` целиком; `[mail]` оставлен дефолтным намеренно (`:20-26`) |
| Session providers herdr / acp / subprocess / exec / k8s | ✘ | herdr брошен 2026-10-26; ACP оценён как «weak» в прошлом исследовании (deja `ab9197254c38f2ecd`: resume пропускается при `tp.IsACP`) |
| Харнесс `pi` | ✘ | `[providers.pi]` объявлен (`city.toml:18`), запусков 0; MCP ему не проецируется (`SV/docs/kb.md:57-58`) |

Поля агента: используются `description, scope, provider, option_defaults, overlay_dir, max_active_sessions,
wake_mode, nudge, env, start_command, lifecycle, work_dir, pre_start, idle_timeout, append_fragments,
prompt_template` — 16 из 57 в `gascity/docs/reference/config.md:80-143` **[ПРОВЕРЕНО]**.

### 2.4 Боли и обходные пути (что пришлось писать самому)

| Боль gc | Что написал владелец | Источник |
|---|---|---|
| Тик reconcile 30 с; записи control dispatcher никого не будят → хоп 22–39 с вместо 5–10 с | `mp_wake.py` (poke в сокет контроллера), `runner.sh` «задерживается» 20 с; replan-lab 322–329 с → 230–243 с | `MP/README.md:2406-2431` **[ПРОВЕРЕНО по документу владельца]** |
| Сессия «busy forever»: gc читает панель, спиннер её обновляет | `mp_liveness`/`mp_kick` (метрополис), `servitors-stall-sweep` (по mtime транскрипта) | `AUDIT:346-355`, `SV/README.md:232-262` |
| Экраны, на которые gc не отвечает (login, usage limit у промпта, model switch, Settings Error); gc о пропущенном диалоге не сообщает | `mp_panes.py` + session-sweep каждые 2 мин (peek панелей); `notice.model_migrations` для Codex | `MP/README.md:2031-2055`, `RULES:744-750`, `SV/pack.toml` |
| herdr не отвечает на стартовые диалоги → pool-сессии висят «активными» | бид `gc-ays`, комментарий в gascity#7027, переход на tmux | deja `a28f8022…`, событие `provider.swapped herdr → tmux` 2026-10-26T01:46 |
| `sling` отказывает epic-биду; drain читает только конвой; конвой не видит бид другого store | input-convoy шим ~450 строк, карта epic→rig tasks, свой live-run check | `AUDIT:137-142`, `MP/README.md:2358-2366,2396-2397` |
| gc оставляет синтетический input-конвой открытым (gascity#5984) | ордер `servitors-convoy-sweep` | `SV/orders/servitors-convoy-sweep.toml:1-5` |
| `gc convoy add` молча игнорирует 2-й и далее id (gascity#8422) | правило в `AGENTS.md` | `CITY/AGENTS.md:67` |
| Formula-гейт — «голый» бид без типа/таймаута; core gate-sweep смотрит только city store | свой gate-sweep каждые 60 с по всем ригам | `AUDIT:122-123` |
| Control-биды (scope-check, finalize, spec) нельзя скрыть (gascity#8272) | метка `metropolis:ctl` постфактум, свои `status`/`ls` | `MP/README.md:2385-2395` |
| Один settings-файл Claude на город; нет переменной с каталогом пака для сессий | диспетчер хуков + симлинк `.gc/runtime/packs/metropolis/pack` | `AUDIT:127-128`, `RULES:735-736` |
| Один `overlay_dir` на агента без наслоения | общий overlay через файловые симлинки + unit-тест на «не копия» | `SV/README.md:117-122`, `RULES:319-325` |
| Pool-сессия стартует в worktree, только если на биде все 9 ownership-ключей | ~600 строк confinement-хуков (metropolis) | `AUDIT:133` |
| Env: нет единого места (`[agent_defaults]` и `pack.toml` без `env`) | pack-провайдер `servitors-claude` как носитель env | `RULES:723-734` |
| Смена MCP или `[workspace] env` меняет fingerprint → рестарт всех сессий | «один reload в тихий момент» | `SV/docs/kb.md:44-46` |
| Прокси сервиса держит lock контроллера на время запроса | deadline 5 с в `serve.py` | `SV/docs/kb.md:213` |
| Codex не раскрывает `${GC_CITY_NAME}` в URL MCP (gascity#9017) | Codex-роли без kb, бид `gcp-isy` | `SV/docs/kb.md:55-57` |
| Старый `gc`/`bd` в PATH ломает город; супервизор под pitchfork | раздел `AGENTS.md`, ритуал bump из 5 шагов | `CITY/AGENTS.md:37-58,72-82` |

**Квоты/429.** В `metropolis.log`, `stuck.log`, `events.jsonl` живых инцидентов нет **[ПРОВЕРЕНО]** (grep по `usage
limit|429|quota|rate limit`). Известные дыры: на диалоге usage-limit при старте gc выбирает «Stop» (сессия
выходит), у промпта — не отвечает; stall-sweep не отличает ожидание лимита от зависания
(`MP/README.md:2039,2049`, `SV/README.md:261`).

**Что ломалось по событиям** **[ПРОВЕРЕНО]**: `session.stranded` 20, `execution.step_stalled` 4,
`bead.claim_rejected` 10, `order.failed` 66, `work.started_unrecorded` 36, `session.demand_claim_divergence` 216.
Сегодня 07:38–07:39 в логе супервизора серия `tmux state cache: refresh failed … no tmux server running`; на
сокете `gc` живут два tmux-сервера, один без дочерних процессов с 25.10 (`ps`).

**Самокритика владельца** (важно для H11): `AUDIT:158-194` — ~8 тыс. строк из 30.7 тыс. лишние, 0 живых
использований у cross-rig вопросов, decision-гейтов, `land=merge`, in-step retry; пак **переизобрёл** то, что в gc
есть (`[steps.retry]`, `gc worktree`, `idle_timeout`/`max_session_age`/`on_death`, event-ордера, `escalate.sh`,
overlay на агента).

### 2.5 Молекулы / виспы / конвои на практике

- **Молекула (v1)** как пользовательская сущность у владельца отсутствует. `CITY/.gc/molecules/` — 2 каталога от 26.10.
- **Граф шагов из формулы** — используется: 48 запусков, 846 `execution.step_defined`, из них начато сессией 199
  (23 %). Остальное — control-биды, пропущенные по `condition`, шаги повторных попыток **[ПРОВЕРЕНО]** (events).
- **Виспы**: 383 session-бида, 9 841 order-tracking, почта. Пользовательской ценности не несут.
- **Конвои**: 24 создано, все синтетические; `convoy.created` 15 событий. Владелец зафиксировал правило: «Convoys are
  gc's, taken as they are… MUST NOT be replaced by … an epic's children: drain, root dedupe, auto-close and
  retire-on-claim read only convoys» (`RULES:166-180`). То есть конвой нужен **движку gc**, а не процессу.

Что останется от формул владельца без molecule/convoy (**[ВЫВОД]**):

| Формула | Суть | Эквивалент «эпик + дочерние биды» |
|---|---|---|
| `servitors.explore` | 1 LLM-шаг | бид с ролью-исполнителем |
| `servitors.plan` | LLM-шаг + human-гейт; затем код создаёт биды задач | бид + блокирующий gate-бид; дочерние биды под задачей |
| `servitors.implement` (+`implement-task`) | fan-out по набору бидов, параллельно, порядок по `blocks`, exclusive | «возьми ready-детей эпика, по одной сессии на бид, не больше N» — нужен планировщик, не конвой |
| `servitors.qa`, `quality-loop` | скрипт гоняет lint/test → LLM чинит → повтор до N | нужна петля с exec-проверкой и счётчиком; из «эпик+дети» сама не получается |
| `metropolis.rig-task` | 20 шагов, 26 vars, 6 режимов | владелец сам назвал «god workflow» и разбил на блоки (`AUDIT:198-209`) |

### 2.6 Ресурсы живого города (снимок `ps` + `/proc/*/smaps_rollup`, 10:10)

| Процесс | Кол-во | RSS | Примечание |
|---|---|---|---|
| `dolt sql-server` | 1 | 817 МБ (10 мин раньше 1.33–1.40 ГБ) + 292 МБ swap | CPU time 3д 17ч за 14.3 сут = 26 % ядра; uptime 14 сут |
| `gc supervisor run` | 1 | 237 МБ (PSS 190) | рестарт ~6 мин назад; 10 % CPU |
| `tmux -L gc` | 2 | 2.3 + 1.9 МБ | один сервер-сирота с 25.10 |
| `claude` (mayor, sonnet) | 1 | 259 МБ | `--resume`, `--settings CITY/.gc/settings.json` |
| `claude` (desk, opus) | 1 | 212 МБ | то же |
| `deja mcp` (stdio) под сессией города | 2 × (node 22 МБ + бинарь 23–26 МБ) | ~45 МБ на сессию | **дубль на каждую сессию** |
| `deja mcp` на машине всего | 4 пары под 4 процессами `claude` | до 1 015 МБ у одной | 2 пары — не городские сессии |
| `kb`: `serve.py` + `bm mcp` (HTTP) | 1 + 1 | 35 + 315 МБ | **один на город** |
| `pitchfork supervisor` + 3 log-sink | 4 | ~115 МБ | обвязка владельца |
| pool-сессии | 0 | — | scale-from-zero; пик за историю — 11 одновременных |

Диск: `CITY/.beads` 301 МБ, `CITY/.dolt-backup` 1.4 ГБ, `session-reconciler-trace` 1.1 ГБ, `events.jsonl` 86 МБ,
`dolt.log` 17 МБ (60 тыс. предупреждений `nothing to commit`). Машина: 12 ГБ / 9 CPU; в первом прогоне был OOM
(`CITY/tmp/test-run-2-prompt.md`, п. 9 чек-листа).

Сессии короткоживущие: по парам `session.woke`/`session.stopped` медиана жизни planner/reviewer/scout 27–82 с,
worker 80 с, runner 10 с **[ПРОВЕРЕНО, но пары woke/stopped сопоставлены приблизительно]**. 468 пробуждений =
468 новых процессов харнесса.

## 3. Вердикты

**H7 (кода много больше, чем используется; большая часть — наследие) — подтверждена частично.**
Первая половина подтверждена измеримо: 10/34 секций, 16/57 полей, ~24/61 команды, 1 провайдер сессий, 0 ACP/pi/k8s.
Вторая («наследие») со стороны пользователя не доказывается: заметная часть неиспользуемого — hosted/enterprise
(login, webhooks, k8s, extmsg, storage classes), а не старьё. И контрфакт: используемая часть (движок formula v2,
claim-протокол, pool-масштабирование, worktree, overlay, pack-команды) нетривиальна, а владелец, не зная про
готовые ручки gc, переписал их сам (`AUDIT:178-194`).

**H6 (молекулы лишние; конвои — переусложнение) — частично.**
Конвои: подтверждено для пользователя — ни одного осмысленного конвоя, только шимы и баги. Но это следствие дизайна
gc (drain/dedupe/retire читают только конвой), а реальная потребность под ним есть: «набор бидов → параллельный
fan-out с порядком по зависимостям и эксклюзивностью». Молекулы v1: не используются. «Граф шагов из шаблона»:
**опровергнуто, что он не нужен** — на нём стоят обе версии процесса владельца. Однако в `servitors` формулы
усохли до 1–3 шагов и бюджета «≤ 8 шагов» (`RULES:27`), а 77 % определённых шаг-бидов никто не исполнял. Вывод:
нужен не шаблонизатор бидов, а 4 примитива (маршрут на роль, fan-out, петля с проверкой, гейт).

**H9 (нет общего MCP, стандартных хуков, слоёного конфига) — в букве опровергнута на 2/3, по сути подтверждена.**
- MCP: реестр-проекция есть (`mcp/` пака → все агенты, `gc mcp list`), общий процесс возможен через
  `[[service]]` + HTTP, владелец так и сделал (решение D5: «10 stdio MCP-серверов на машину недопустимы… один
  HTTP-сервер на город», `PACKS/tmp/kb-plan.md:34`). Не решено: stdio-MCP плодятся на сессию (deja), нет opt-out по
  агенту, правка MCP рестартит все сессии, Codex/pi получают не всё, прокси блокирует контроллер.
- Хуки: подтверждено. gc ставит только свои; автору пака — три разных способа на два харнесса, для pi — ничего.
- Слоёный конфиг: опровергнуто в букве (цепочка `builtin:claude → [providers.claude] → pack-провайдер → agent`,
  `[agent_defaults]`, `gc config explain`). Дыры: env, overlay, «pack-провайдер берётся, только если в городе нет
  одноимённого»; `explain` у всех полей показывает источник `city.toml`.

**H11 (переписать с нуля) — со стороны использования поддерживается, с двумя оговорками.**
За: обязательный набор мал и чётко очерчен; главные боли (Dolt, тик, скрейпинг панели, конвой как обязательная
обёртка) конфигом не лечатся. Оговорка 1: часть болей — свойства харнессов, а не gc (правила загрузки
`settings.json`/`settings.local.json`, доверие к хукам Codex по хэшу, плагины, диалоги логина), они переедут в
любую систему. Оговорка 2: владелец уже прошёл цикл «30 тыс. строк за 8 дней → выбросить»; риск повторить его в
новой системе выше риска «не хватит фич».

**Попутные данные для чужих гипотез.** H1: tmux стоит 2 МБ, его цена — поведенческая (скрейпинг диалогов,
«busy» по спиннеру, nudge набором текста). H4: 212–259 МБ на сессию claude + 45 МБ stdio-MCP; сессии живут
десятки секунд. H5: Dolt — самый дорогой процесс города и главный потребитель LLM-токенов через `bd.dog`. H8:
97.8 тыс. срабатываний ордеров против 199 шагов работы; все ордера владельца — поллинг. H10: оператор входит через
`gc session attach desk`; в новом паке решено «mayor — сессия оператора, desk нет» (`RULES:17`).

## 4. Следствия для дизайна и варианты

Минимальный набор, чтобы город переехал без потерь.

**Must**
1. Риги с префиксами бидов, **один эпик над бидами нескольких ригов** (сегодня это главная причина шимов).
2. Бид-стор: типы, parent-child, `blocks`, комментарии, метки, **произвольная metadata KV**, атомарное создание
   графа, конкурентные писатели, `ready`-запрос. Без LLM-обслуживания хранилища.
3. Роли: харнесс + модель + effort + env + права на роль; `fresh`/`resume`; pool с лимитом и scale-from-zero.
4. **Скриптовые агенты** без LLM в том же claim-цикле (у владельца 143 из 458 стартов).
5. Claim-протокол: атомарный захват, возврат при смерти сессии, «один claim на разговор» для fresh-ролей.
6. Маршрут шага на роль; зависимости; условие; **петля «exec-проверка → повтор ≤ N»**; **fan-out по набору бидов**
   (параллельно, порядок по `blocks`, exclusive); scope с abort и cleanup-шагом.
7. Гейты: human и «ждать внешнего условия», с типом, таймаутом и причиной отказа на самом гейте.
8. Worktree на бид/задачу, сессия стартует прямо в нём.
9. Pack-расширения: свои команды, промпт-шаблоны и фрагменты, vars рига (`format_cmd`/`lint_cmd`/`test_cmd`).
10. Хуки харнесса, объявляемые **один раз на роль** и транслируемые в claude/codex/pi; встроенные: prime,
    handoff при компакции, инъекция почты.
11. Общий на город MCP (один процесс) + opt-in/opt-out по роли; правка MCP не рестартит весь город.
12. Liveness по транскрипту/протоколу, а не по экрану: stall → interrupt → reset → defer; явные состояния
    «ждёт лимита», «ждёт логина», «ждёт ответа пользователя».
13. Событийное пробуждение следующего шага (целевой хоп ≤ 5 с без poke-костылей).
14. Сессия оператора (attach/resume) и почта «роль → mayor/human» с dedup.

**Should**: периодические задания (cron/cooldown) и реакция на событие с payload в env; usage по ролям; слоёный
конфиг с честным provenance; doctor-проверки пака; сервисы пака (HTTP за одним портом); offsite-бэкап стора.

**Won't need** (по данным города): конвой как пользовательская сущность; молекулы v1; herdr/subprocess/k8s/exec
провайдеры; extmsg, webhooks, github monitors, converge, hosted login, storage classes, agent-script, build-image;
9 dolt-ордеров и `bd.dog`; order-tracking биды; control-биды в пользовательском сторе; tmux-socket/alias-поля.

Варианты под главную развилку H6:
- **A. «Эпик + дети», шаги в голове агента.** Формула = промпт роли + финишер-команда. Покрывает explore/plan/
  implement-task. Не покрывает петли lint/test и fan-out — их делает скриптовый агент.
- **B. Тонкий граф** (4 примитива из Must-6), шаги — обычные дочерние биды эпика, без control-бидов и конвоев.
  Покрывает всё из `servitors`. Это ближе всего к тому, к чему владелец уже пришёл сам.
- **C. Полный formula v2.** Нужен только `metropolis.rig-task`, который владелец заморозил.

## 5. Открытые вопросы и непроверенное

- **[НЕ ПРОВЕРЕНО]** Насколько `usage.jsonl` полон: все записи `unpriced`, атрибуция по `worker`. Доля `bd.dog`
  может быть завышена, если у части сессий usage не собирался.
- **[НЕ ПРОВЕРЕНО]** Число бидов в самих сторах: считал по `bead.created` в шине событий, не запросом к Dolt
  (не трогал живой Dolt). События rig-сторов могли попасть не все.
- **[НЕ ПРОВЕРЕНО]** Использует ли владелец dashboard глазами; в коде и docs он лишь упоминается.
- **[НЕ ПРОВЕРЕНО]** Причина серии `no tmux server running` сегодня в 07:38 и происхождение второго tmux-сервера.
- Можно ли в одном ACP-процессе держать роли с разным env/overlay/правами (у владельца ≥ 6 разных наборов
  настроек на Claude-роли: lean/не lean, `CLAUDE_CODE_DISABLE_CLAUDE_MDS`, browser-env, read-only deny) — вопрос к
  отчётам по ACP; от ответа зависит реальная экономия по H4.
- Пиковая нагрузка города не наблюдалась (сейчас 2 сессии); цифры RSS pool-сессий взяты с named-сессий.
- Город — dev-полигон с `dry_run="true"`; требования «боевого» города (реальные PR, merge-очередь, несколько
  операторов) этим отчётом не покрыты.

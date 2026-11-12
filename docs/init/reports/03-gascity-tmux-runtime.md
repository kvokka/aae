# 03 — Что tmux делает в Gas City и что придётся заменять без него (H1)

Срез: `gascity` @ f1f75f53c (2026-11-10). Строки — `wc -l` по не-тестовым `.go`; диапазоны внутри
`tmux.go` — по границам функций. Метки: **[П]** = ПРОВЕРЕНО лично (код/вывод/документ),
**[П-С]** = найдено делегированным read-only обходом кода, мной перепроверено выборочно,
**[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО. Пути без префикса — от корня `gascity`;
`src/…` — от `tmp/gc-alt-research/src/`.

## 1. Выводы за 60 секунд

- **tmux унаследован буквально.** `internal/runtime/tmux/tmux.go:32-35`: «This file was copied from
  github.com/steveyegge/gastown internal/tmux/tmux.go … (2026-02-22)». За 8,5 месяцев файл вырос
  с 2442 до 6923 строк, пакет — с ~2,6k до 20 400 не-тестовых строк (+48 309 тестовых). **[П]**
- **tmux несёт 16 обязанностей; «общаться с агентом» — только одна.** Примерно половина — реальные
  потребности системы (запуск, kill дерева процессов, liveness, HITL), вторая половина — артефакт
  того, что агент — TUI: send-keys-гонки, скрейпинг экрана, диалоги, fence, activity-часы. **[П]**
- **Три вещи tmux даёт бесплатно, и без него их придётся строить:** (1) агент переживает рестарт
  оркестратора, (2) человек может `attach` и вмешаться, (3) работает любой CLI. Первое — самое
  весомое: на живом городе `gc supervisor` имеет uptime 8 минут, а агенты под tmux-сервером
  (PPID 1) — 42 минуты. **[П]**
- **ACP-провайдер Gas City первого свойства не имеет:** агент — дочерний процесс на stdio-пайпах
  (`internal/runtime/acp/acp.go:281-304`), `session/load` и `session/resume` в не-тестовом коде не
  вызываются вообще, `Attach` возвращает ошибку (`acp.go:764`). Это недоработка Gas City: в схеме
  ACP v1 оба метода есть. **[П]**
- **Квота/429 сейчас ловятся двумя путями:** скрейпинг экрана мёртвого pane
  (`internal/runtime/dialog.go:1902-2012`) и — с ноября 2026 — разбор transcript харнесса
  (`internal/sessionlog/turns_*.go`, «no text is read»). Тезис владельца «логи так и так есть» верен:
  Claude пишет `isApiErrorMessage`+`error:"rate_limit"`, Codex — `codex_error_info`. **[П]**
- **Но «всё решается через ACP» — неточно.** В ядре ACP нет типизированного сигнала квоты/429:
  `StopReason` = `end_turn|max_tokens|max_turn_requests|refusal|cancelled`; RFD прямо откладывает
  «rate limits and quotas» на будущее. Типизированы только auth (`-32000`), permission, конец turn.
  Квота приходит вендорским `_meta` (`_claude/rateLimit` в claude-agent-acp). **[П]**
- **Реакция на сбои бедная:** quarantine 30 мин по экрану, один fresh restart на эпизод, алерт.
  Переключения аккаунта/провайдера в Go нет (в Gas Town был `internal/quota` с ротацией аккаунтов —
  в Gas City не перенесён). Истёкший auth в рантайме не детектится вовсе. **[П-С]**
- **Кода «только из-за TUI в терминале» ≈ 20–26k не-тестовых строк** (≈2–3 % от ~930k), плюс ~10k
  смешанного (nudge-машинерия). По строкам это не главный источник раздутости; по багам — главный:
  59 % коммитов в `internal/runtime/tmux` — fix (161 из 271), 22 % всех issues упоминают tmux
  (452 из 2040). **[П]**
- **Авторы не уходят от терминала — они уходят от скрейпинга.** Курс: OSC 7501 Program Status
  Protocol (харнесс сам сообщает `idle/working/blocked` escape-последовательностью) + другие
  мультиплексоры (herdr, Rex). Обоснование записано: «tmux is the universal transport and acp is
  premium» (`engdocs/design/worker-runtime-transport-v0.md:346-373`). **[П]**
- **Неожиданное:** claude 2.1.322 и codex 0.166.0 уже содержат эмиттер OSC 7501 (строки в бинарях),
  pi — через `PI_PROGRAM_STATUS=1`. То есть «tmux без скрейпинга» — реальная альтернатива, а не
  только «tmux против ACP». В Gas City переключатель по умолчанию `off`. **[П]**
- **Вердикт H1: частично подтверждена.** «Унаследован» — да. «С нуля под claude/codex/pi не нужен
  как транспорт» — да. «Всё решается через ACP» — нет: квота/429 требуют per-harness адаптера, а
  выживание процесса и attach — отдельной конструкции, которую tmux давал даром.

## 2. Факты

### 2.1 Происхождение и масштаб

| Факт | Источник |
|---|---|
| `tmux.go` скопирован из gastown `a4387800b619`, коммит `307e5a14a` (2026-02-22), 2442 строки | `tmux.go:32-35`; `git show 307e5a14a --stat` **[П]** |
| Остатки Gas Town в API: `ConfigureGasTownSession`, `SetCrewCycleBindings`, `SetTownCycleBindings` | `tmux.go:6403, 6577, 6584` **[П]** |
| Пакет tmux: 20 400 / 48 309 (код/тесты). Для сравнения acp 6 119 / 11 484, subprocess 1 432 / 2 684, herdr 4 265 / 7 589 | `wc -l` **[П]** |
| Дизайн-док herdr оценивал tmux-провайдер в «~290 LOC», а herdr — в «~300–500 LOC» | `internal/runtime/herdr-provider-design.md:206, 312` **[П]** |
| 289 не-тестовых Go-файлов упоминают tmux (117 в `internal/runtime`, 81 в `cmd/gc`, 19 в `internal/api`) | `rg -l -i tmux` **[П]** |
| В Gas Town tmux-пакет 5 581 строка, его импортируют 97 файлов; есть `internal/acp` (1 698, proxy для `gt mayor acp`) и `internal/quota` (1 412, скан pane + ротация аккаунтов) | `src/gastown/internal/{tmux,acp,quota}` **[П]** |

### 2.2 Обязанности tmux и окружающего кода

| # | Обязанность | Где (не-тест) | Объём | Потребность системы или артефакт TUI |
|---|---|---|---|---|
| 1 | Запуск процесса, cwd, env, setup-команды, prompt-файл, секреты не в argv | `tmux.go:776-1097`; `adapter.go:110-165, 1590-2050, 2150-2265, 2647-2872`; `session_env_file.go`; `internal/runtime/envsecret.go` | ~1,5k | Потребность. Через tmux — усложнение: env в `-e K=V` виден в `/proc/*/cmdline` (`envsecret.go:1-8`) |
| 2 | Процесс переживает смерть контроллера | tmux-сервер — демон (PPID 1); цена наблюдения за чужим демоном: `state_cache.go` 1365, `server_socket_probe*.go` 553 | 0 + ~1,9k | Потребность. **tmux даёт бесплатно** |
| 3 | Kill дерева процессов, orphan sweep | `tmux.go:1098-1700`; `pane_subtree*.go` 281; `proctable/` 2425 | ~3,3k | Потребность, остаётся |
| 4 | Доставка ввода (nudge): literal/paste-buffer, Escape/Enter per-provider, подтверждение submit по busy-индикатору, очистка композера, hidden-attach клиент, copy-mode | `tmux.go:2138-2285, 2395-4065`; `composer_clear.go` 500; `submit_turn_guard.go` 421 | ~2,7k | Потребность «доставить сообщение». Реализация — **артефакт** |
| 5 | Input fence: не напечатать в чужую инкарнацию pane | `tmux/fence.go` 757; `internal/runtime/tmux_fence.go` 184; `carrier_fence.go` 306 | ~1,2k | **Артефакт** (в протоколе соединение = инкарнация) |
| 6 | Чтение экрана: ready/idle/busy | `tmux.go:5056-5118, 5385-6297`; `internal/runtime/idle_boundary.go` | ~1,1k | **Артефакт**. Busy = подстроки `"esc to interrupt"` и regex спиннера (`tmux.go:6206-6217`) |
| 7 | Стартовые и mid-session диалоги: trust, theme, resume, update, MCP, hooks, bypass, API key, feedback survey, model switch | `internal/runtime/dialog.go` 2225; `dialog_midsession.go` 210; `tmux.go:4066-4365`; `adapter.go:2266-2412` | ~2,9k | **Артефакт** |
| 8 | Permission prompt / AskUserQuestion: распознать и ответить клавишами | `tmux/interaction.go` 1152, `interaction_codex.go` 522, `interaction_question.go` 449 | 2,1k | Потребность (HITL). Реализация — **артефакт** |
| 9 | Детект квоты/лимита/terminal error по тексту экрана | `dialog.go:1896-2130` | ~0,25k | Потребность. Реализация — **артефакт** |
| 10 | Liveness, zombie (pane жив, агент мёртв), `remain-on-exit` | `tmux.go:4366-4765, 4895-5055`; `internal/runtime/liveness.go` 236 | ~0,8k | Потребность; на stdio это `wait(2)`/EOF |
| 11 | Activity-часы для hang-детекта | `tmux.go:4766-4894`; `internal/runtime/tmux_activity.go` 194 | ~0,3k | Потребность. Реализация — **артефакт**: вычитать собственные нажатия gc (`tmux_activity.go:12-19`) |
| 12 | Interrupt/stop turn: Ctrl-C, ожидание `<turn_aborted>` в rollout Codex, остановка фоновых shell через TUI | `adapter.go:352-400`; `tmux.go:5992-6205`; `claude_background_stop.go` 364; `codex_background_stop.go` 275 | ~0,9k | Потребность. Реализация — **артефакт** |
| 13 | Attach человека + «не затереть ввод человека» | `adapter.go:1464-1497`; `tmux.go:2286-2394`; `runtime.go:42-48` | ~0,2k | Потребность. **tmux даёт бесплатно** |
| 14 | Meta KV (drain, fingerprint, restart-request) в tmux environment | `adapter.go:981-1022` | ~0,1k | **Артефакт** (место — bead или файл) |
| 15 | Топология pane, status bar, темы, биндинги | `tmux.go:4393-4587, 6298-6897`; `theme.go`; `internal/runtime/tmux_agent_pane.go` | ~0,9k | UX/наследие Gas Town |
| 16 | OSC 7501: tap потока pane, relay, shadow-сравнение со скрейпом | `tmux/program_*.go` 2741; `internal/runtime/program_status*.go` 1344; `programstatus/` 2806; `cmd/gc-osc7501` 223; `cmd/gc/*program_status*` 733; `internal/config/program_status.go` 261 | 8,1k | **Артефакт «агент в терминале»**: структурный сигнал, протянутый сквозь TTY |

Все строки таблицы **[П]**. Нагляднее всего артефакт виден в `nudgeSession`
(`tmux.go:3587-3935`, ~350 строк на одну доставку сообщения): lock → найти agent pane → выйти из
copy-mode → SIGWINCH отсоединённому pane → подождать idle → проверить permission prompt → закрыть
mid-session диалог → C-u (только если человек не attached) → закрыть feedback survey → paste →
Escape (не для 13 имён провайдеров, `tmux.go:54`) → Enter → poll busy-индикатора → retry. Комментарий
`tmux.go:83-99` честно фиксирует, что причина «текст вставлен, но не отправлен» у Claude **не
найдена**. Стартовый nudge повторяется лесенкой 2/4/6/8 с, потому что «readiness detection and the
TUI actually accepting input are not the same moment» (`adapter.go:2414-2421`). **[П]**

### 2.3 Чем заменяется в мире без tmux

| Обязанность | Структурный протокол (ACP / stream-json / app-server) | Итог |
|---|---|---|
| 4 доставка ввода | `session/prompt`; ответ запроса = подтверждение доставки | **Исчезает полностью** |
| 5 fence | соединение/sessionId | **Исчезает** |
| 6 idle/busy | ответ на `session/prompt` (stopReason) + `session/update` | **Исчезает**; остаётся watchdog «turn длится > N» |
| 7 диалоги | нет TUI — нет диалогов; trust/permission mode задаются параметрами | **Исчезает** (кроме первичного login) |
| 8 permission/question | `session/request_permission` (в Gas City уже есть: `internal/runtime/acp/permission.go:474-507`) | Остаётся потребность, реализация тривиальна |
| 9 квота/429 | **в ядре ACP нет** (см. 2.4); вендорский `_meta` + transcript | Остаётся; нужен per-harness адаптер |
| 10 liveness | exit дочернего процесса / EOF | Упрощается на порядок |
| 11 activity | поток `session/update` | Упрощается, точнее |
| 12 interrupt | `session/cancel` (в Gas City: `acp/interrupt.go` 303) | Упрощается |
| 14 meta, 15 топология, 16 OSC 7501 | не нужны | **Исчезают** |
| 1, 3 запуск/kill | `exec` + process group | Остаётся, проще |
| **2 выживание при рестарте оркестратора** | **нет аналога**: stdio-ребёнок умирает с родителем | **Строить заново**: отдельный broker-демон либо «убить и `session/load`» |
| **13 attach** | **нет аналога**: `acp.go:764` → `"acp provider does not support attach"` | **Строить заново**: свой клиент/UI или `claude --resume <id>` в терминале |
| универсальность (любой CLI) | только харнессы с протоколом | Теряется; для claude/codex/pi не важно |

Авторы формулируют ту же развилку: «tmux externalizes persistence to a daemon… acp's process IS the
session… tmux sessions survive a controller restart… acp streams do **not** survive unless the
Runtime brokers/daemonizes the process» (`engdocs/design/worker-runtime-transport-v0.md:348-369`). **[П]**

### 2.4 Обработка сбоев сейчас

Механизмы: (1) текст экрана, (2) transcript харнесса, (3) process table / exit status, (4) протокол.

| Сбой | Как узнаёт | Что делает | Без tmux |
|---|---|---|---|
| **а) квота подписки / usage limit** | (1) экран мёртвого pane: `"insufficient_quota"`, `"quota exceeded"` (но не `"disk quota"`), `"credit balance too low"` — `dialog.go:1990-2012, 2041-2064` **[П]**. (2) только Codex: `codex_error_info=="usage_limit_exceeded"` — `internal/sessionlog/turns_codex.go:90-97` **[П-С]** | `sleep_reason=provider-terminal-error`, unhealthy, **без self-heal** — нужен оператор (`cmd/gc/session_reconcile.go:739`). Work ladder: алерт `rate_limited_quota`, seat «paced paused» **[П-С]** | claude: `_meta["_claude/rateLimit"]` с `resetsAt` (`src/claude-agent-acp/src/acp-agent.ts:8739-8752`) и типы сбоев `quota_exhausted`, `rate_limited`, `overloaded`, `auth_required`, `context_exhausted` (`src/session-failure-extension.ts:19-32`) — **вендорское расширение, не стандарт** **[П]** |
| **б) 429 / overloaded** | (1) `"Usage limit reached"`, `"You've hit your limit"`, `"/rate-limit-options"`, `"rate limit"`+`"Keep trying"`+`"Stop"` — `dialog.go:1946-1958`, последние 120 строк pane (`cmd/gc/session_types.go:369`) **[П]**. (2) Claude `isApiErrorMessage`+`error=="rate_limit"`; Codex HTTP 429 **[П-С]**. (3) exit code 75 до конца старта → `CapacityError` (`runtime.go:76-113`) **[П]** | (1) quarantine 30 мин без счёта краша (`session_types.go:359`) **[П]**. (2) 2 отказа подряд → пауза, после 10 мин **один** fresh restart на эпизод, потом алерт (`cmd/gc/work_ladder_ratelimit.go:1-35`) **[П]**. (3) circuit breaker per endpoint (`cmd/gc/endpoint_capacity.go`). `"overloaded"`/529 — только сырой код, реакции нет **[П-С]** | JSON-RPC error на `session/prompt` (текст) либо vendor `_meta`; transcript остаётся запасным источником |
| **в) краш харнесса** | (3) `remain-on-exit` оставляет «труп» pane; liveness = pane жив × процесс из `GT_PROCESS_NAMES` жив; `ErrSessionDiedDuringStartup` (`runtime.go:72-74`) **[П]** | Смерть <30 с = rapid crash, 5 подряд → quarantine 5 мин (`session_types.go:320, 352`) **[П]**. In-memory crash tracker, сбрасывается рестартом контроллера (`cmd/gc/crash_tracker.go:1-4`) **[П]**. Рестарт с resume: claude `--resume <uuid>` (ключ генерит сам gc через `--session-id`), codex `resume`, pi `--session` (`internal/worker/builtin/profiles.go:267-275, 361, 901`) **[П]**. 3 неудачных resume → fresh (`session_reconcile.go:819`) **[П-С]** | exit/EOF мгновенно, без чтения трупа; resume = `session/load` |
| **г) зависание** | (3) `#{window_activity}` минус собственные нажатия; (1) busy-скан, доверенный только для claude/codex (`tmux.go:3355, 3366-3378`); для pi ответ — `ErrIdleIndeterminate` **[П]** | idle timeout → sleep/recycle; nudge с backoff 3→60 мин; `claim_stall_guard` (888 строк); opt-in `progress_stall_timeout` **[П-С]** | watchdog по состоянию turn, а не по «тишине терминала» |
| **д) ждёт ввода/разрешения** | (1) парсинг `"Do you want to "`, нумерованных меню, Codex overlays (`tmux/interaction.go:254, 728`); polling раз в 5 с (`cmd/gc/pending_interaction_lane.go:26`) **[П-С]**. (4) ACP `session/request_permission` | Событие `session.pending`; nudge отклоняется (`runtime.go:61-65`); отвечает человек через `Respond` | push-запрос от агента, ответ по id |
| **е) контекст/compaction** | (2) hook UserPromptSubmit читает usage из transcript, подсказка >60 %, «hand off» >80 % (`cmd/gc/context_inject.go:15-45`); (3) «context-churn» по времени жизни **[П-С]** | PreCompact-hook → `gc handoff --auto` (письмо себе); факт compaction записывается, реакции нет **[П-С]** | `usage_update`, `stopReason: max_tokens` |
| **ж) auth истёк** | **Рантайм-детекта нет.** Только preflight `claude auth status --json` при init (`internal/api/handler_provider_readiness.go:388`) **[П-С]**; мой `rg` по `/login`, `authentication_error`, `oauth token` в dialog/tmux/turns/reconcile — пусто **[П]** | Косвенно: stall-guard и progress-stall | ACP: типизированная ошибка `-32000 Authentication required` (`src/agent-client-protocol/schema/v1/schema.json`) **[П]** |

Про протокол: `src/agent-client-protocol/docs/rfds/session-usage.mdx:207-209` — «Rate limits and
quotas are separate concerns that could be addressed in a future RFD»; `docs/rfds/v2/prompt.mdx:320` —
«More specific categories, such as rate limits, an exhausted context window, or an overloaded
provider, can be added later». **[П]**

Failover аккаунта/провайдера в Go отсутствует сознательно: «The reconciler is a pure consumer: it
never classifies errors itself» (`cmd/gc/provider_health_gate.go:1-5`) — решение вынесено в pack
(`anthropic-failover-watcher` пишет `provider-health.json`). **[П]** У pi в `sessionlog` нет парсера
turn (`internal/sessionlog/turns.go:86-98`: только claude, codex, gemini) — для pi transcript-путь
детекта лимитов не работает. **[П]**

### 2.5 Надёжность: что чинят снова и снова

- 7768 коммитов, 3920 начинаются с `fix` (50 %). В `internal/runtime/tmux` — 271 коммит, из них 161
  fix (59 %); 137 из 271 — после 2026-08-01, то есть темп не падает. **[П]**
- С 2026-10-01: 323 из 1374 коммитов (23,5 %) упоминают
  nudge/send-keys/dialog/paste/composer/capture-pane/trust. **[П]**
- Тесты по именам файлов: `*nudge*` 56 файлов / 27 288 строк, `*fence*` 48 / 16 095,
  `*program_status*` 34 / 10 151, `*idle*` 24 / 7 480, `*dialog*` 11 / 5 132. **[П]**
- Issues: tmux — 236 open / 216 closed из 2040; send-keys 28/19; capture-pane 16/22; acp 32/19;
  herdr 30/14 (`gh api search/issues`). **[П]**
- Повторяющиеся классы **[П-С, примеры П]**: (i) матчер текста срабатывает на старом scrollback —
  его сужают до «на одной строке», «в окне 6 строк», «footer на последней строке» (`dialog.go:1968-1980`);
  (ii) нажатия попадают не туда: nudge отвечает на диалог, «0» закрытия survey уходит как prompt
  (#6859, open); (iii) busy/idle: #5692 (open) — «submit Enter delivered to tmux but not confirmed»
  копится на всех pool-сессиях, дубли 3–4 nudge; #6748 (open) — тишина tmux прочитана как idle,
  сессия drained через 47 минут работы с незакоммиченным кодом; (iv) новые варианты диалогов
  Claude/Codex с каждым релизом харнесса.
- Гарантия «нет неохраняемых send-keys» в документации помечена «**Unmet on main under both
  reconcilers**» (`engdocs/architecture/reconciler-v2.md:554`). **[П]**

### 2.6 Сколько кода существует только из-за TUI в tmux

| Группа | Файлы | Строк |
|---|---|---|
| A. Диалоги и скрейп экрана | `internal/runtime/dialog.go`, `dialog_midsession.go` | 2 435 |
| B. Permission/question парсеры | `tmux/interaction*.go` | 2 123 |
| C. Ввод: композер, submit guard, fence, фоновые shell | `tmux/composer_clear.go`, `submit_turn_guard.go`, `fence.go`, `claude_background_stop.go`, `codex_background_stop.go` | 2 317 |
| D. TUI-сегменты `tmux.go` (send-keys, hidden attach, paste/submit, nudge, диалоги, activity, capture, ready/idle/busy) | диапазоны из 2.2 | 3 334 |
| E. Общие tmux-хелперы | `internal/runtime/tmux_*.go` (498), `carrier*.go` (448), `idle_boundary.go`, `interaction_fence.go`, `startup_nudge_handback.go` | 1 128 |
| F. Стартовая лесенка | `tmux/adapter.go:2266-2536` | 271 |
| **Итого «чистый артефакт»** | | **≈ 11,6k** |
| G. OSC 7501 | см. строку 16 в 2.2 | 8,1k |
| H. Другие терминальные бэкенды | `internal/runtime/herdr` 4 265, `ssh` 2 133 | 6,4k |
| **Итого A–H** | | **≈ 26k** |
| I. Остаток пакета tmux (реальные потребности, дорогая реализация) | 20 400 − B − C − D − F − tmux-часть G | ≈ 9,6k → заменяется на 1,5–6k (subprocess 1 432, acp 6 119) |
| J. Смешанное: nudge-машинерия | 14 файлов `cmd/gc/*nudge*` 9 368 (один `cmd_nudge.go` — 3 981) + `internal/nudgequeue` 1 377 | 10,7k; очередь нужна и без tmux, per-session poller и «unconfirmed»-ветки — нет **[В]** |

Числа **[П]** (`wc -l`), разнесение по группам — **[В]**. Не учтены: tmux-ветки в `cmd/gc`
reconciler, `internal/sessionlog` (17 011 — нужен при любом транспорте), k8s/exec (tmux-в-контейнере).
Нижняя граница ≈ 20k, верхняя с учётом J и переписывания I ≈ 45k не-тестовых строк и ещё
~100k тестовых. От ~930k (CONTEXT.md) это 2–5 %.

### 2.7 Что говорят авторы

- **Почему tmux — дефолт:** «**Posture:** tmux is the default for long-lived/background workers and
  the only transport guaranteed on every Runtime Provider; acp is the premium transport for
  interactive/structured-interaction workers» (`worker-runtime-transport-v0.md:371-373`). **[П]**
- **ACP с марта считается «работающим, но без propulsion»:** issue #23 (open, 2026-03-17, 0
  комментариев): «The goal is **not** "make ACP work" — ACP already works… controller automation
  over ACP is just "send a prompt and hope the surrounding lifecycle works out"». При этом open:
  #1069 и #6080 (nudge poller/dispatcher не доставляет в ACP-сессии), #5218 (утечка ACP-процессов),
  #6081 (убитые ACP-сессии держат claims). **[П]**
- **herdr, «promote only when it's boring»:** док называет его «NOT a tmux replacement — an additive
  backend» (`herdr-provider-design.md:157`), советует «pin the **mayor to tmux** first (don't run the
  orchestrator on an unproven runtime — a wedge can't self-recover)» (`:152-153`), и заканчивает:
  «Pilot on one low-stakes agent…; watch liveness/wedge detection specifically; promote only when
  it's boring» (`:308-309`). Смысл: продвигать в дефолт, только когда перестанет порождать
  инциденты. Пока не перестал: open #4914 (0 агентов материализуется), #5273 (флаг удалён в herdr
  0.8.0), #8934 (сессии путают env при конкурентном старте, 2026-11-06). Мотив herdr —
  «Structured JSON socket API + error codes vs scraping tmux text output (brittle)» и push-события
  вместо polling (`:287-290`). **[П]**
- **Направление — OSC 7501, а не уход от терминала:** issue #8903 (2026-11-06), ruling техлида:
  «**tmux-in-the-box is a convention, not a contract**»; коллаборатор 2026-11-09 начинает runtime
  pack для herdr-core, который «tracks OSC 7501 itself and there's no tmux in the box». Переключатель
  `[session] program_status` = `off` по умолчанию (`docs/reference/config.md:903`); в живом городе
  владельца не задан, `provider = "tmux"` (`/home/ubuntu/proj/share/gc/city.toml:104`). **[П]**
- В `ROADMAP.md` слово tmux не встречается; плана смены дефолта нет. **[П]**

### 2.8 Адвокат дьявола: аргументы за tmux

| Аргумент | Вес в 2026 для claude/codex/pi | Обоснование |
|---|---|---|
| Агент переживает рестарт/краш/апгрейд оркестратора | **Высокий** | `ps`: supervisor 8 мин, агенты 42 мин, tmux-сервер PPID 1. Без tmux: либо broker-демон (новый компонент со своими отказами), либо resume — рестарт процесса, потеря in-flight turn и tool call, промах prompt cache **[В]**. Примечательно: у codex уже есть свой `app-server-daemon` (PPID 1, uptime 23 дня в `ps`) **[П]**, что он делает — **[НП]** |
| Attach: посмотреть и вмешаться руками | **Средний–высокий** | В ACP-провайдере нет вообще. Строить клиент. Дешёвый суррогат: открыть ту же сессию в TUI через `--resume`, когда оркестратор её не держит **[В/НП]** |
| Подписочный TUI без API-ключа | **Низкий технически, средний как policy-риск** | claude-agent-acp работает через логин CLI и по умолчанию биллит подписку; существует флаг `--hide-claude-auth` — «this integration must never bill a claude.ai subscription» (`src/claude-agent-acp/src/hide-claude-auth.ts:1-6`), то есть для части интеграторов это запрещено. Условия для самодельного оркестратора — **[НП]** |
| Отладка «глазами» | Средний | Замена — JSON-RPC capture (`internal/runtime/acp/capture.go`, 390 строк) + transcript; информативнее экрана, но нужен просмотрщик |
| Работает с любым CLI | **Низкий** для владельца | В `tmux.go:54` 13 имён (11 семейств); upstream это нужно, владельцу — три |
| Интерактивные фичи TUI | Средний | README claude-agent-acp:21 — `/mcp reconnect/enable/disable` «Claude Code refuses them in SDK mode», адаптер обходит через control API. Полный список расхождений — **[НП]** |
| TUI уже даёт структурный статус (OSC 7501) + transcript | **Средний, недооценён** | Скрейп idle/busy/blocked уходит и без смены транспорта. Не уходит: send-keys для ввода, диалоги, fence |
| Зрелость | Средний | tmux-путь — «известное зло» с 20k строк страховок; ACP-путь в Gas City — 51 issue и нет resume |

## 3. Вердикты

**H1 — частично подтверждена.**
- *«tmux — унаследованный способ»* — **подтверждено** (2.1): файл скопирован, имена Gas Town живы.
- *«При написании с нуля не нужен»* — **подтверждено для роли транспорта** при трёх харнессах.
  Исчезают обязанности 4–7, 14–16 целиком (≈20k строк и основной поток fix-коммитов); 8, 10–12
  упрощаются.
- *«Всё, включая outOfQuota/429/краш/восстановление, решается через ACP»* — **уточнить**:
  краш, зависание, permission, auth, границы turn — да, типизировано протоколом или ОС.
  Квота/429/overloaded — **нет в ядре ACP**; нужен per-harness адаптер (vendor `_meta`, текст
  ошибки, transcript). Восстановление — методы в ACP есть, но Gas City их не вызывает.
- *Оговорка, которой в гипотезе нет:* tmux — не только транспорт, но и **супервизор процессов,
  независимый от оркестратора**, и **готовый UI**. Эти две роли надо спроектировать явно.

**H2 (попутно).** По моей части ACP в Gas City слаб из-за Gas City: нет `session/load`, нет attach,
nudge-диспетчеры не обслуживают ACP (#1069, #6080), issue #23 без движения 8 месяцев. Слабость самого
протокола, найденная мной, одна — нет стандартного сигнала квоты.

**H7 (попутно).** TUI-наследие даёт 2–5 % строк. Раздутость в 930k объясняется не им; им объясняется
непропорциональная доля багов.

**H8 (попутно).** tmux-детект весь на polling: 100 мс (`tmux.go:43`), pending lane 5 с, patrol 30 с;
сам herdr-док: «gascity **polls** today for liveness/wedge detection» (`:288`).

## 4. Следствия для дизайна и варианты

1. **Вариант A — stdio-ребёнок + resume.** Оркестратор сам порождает `claude-agent-acp`/`codex`/`pi`
   и при своём рестарте делает `session/load`. Минимум кода. Цена: рестарт оркестратора обрывает
   все активные turn; нужна идемпотентность доставки (ключ на prompt) и политика «не рестартовать
   при живых turn».
2. **Вариант B — broker-демон.** Тонкий долгоживущий процесс держит stdio харнессов и отдаёт их по
   unix-socket; оркестратор переподключается. Воспроизводит свойство tmux №2 и даёт точку для
   attach (второй клиент к тому же соединению). Цена: новый компонент, который должен быть «скучнее»
   оркестратора — та же логика, что у «pin mayor to tmux».
3. **Вариант C — терминал без скрейпа.** Оставить tmux, включить OSC 7501 и transcript-детект
   (путь upstream). Сохраняет attach и выживание даром; не убирает send-keys, диалоги и fence
   (≈10k строк и главные open-баги #5692, #6859). Разумен только как форк, не как «с нуля».
4. **Обязательное при любом варианте:**
   - **Failure taxonomy per harness** — малый адаптер, приводящий vendor-сигналы к
     `rate_limited{resetsAt} | quota_exhausted | overloaded | auth_required | context_exhausted |
     crashed`. Образец классов — `session-failure-extension.ts:19-32`. Для pi источник сигнала
     надо найти: в Gas City его нет.
   - **Политика реакции, которой нет сейчас:** сон до `resetsAt` вместо фиксированных 30 минут;
     failover аккаунта/провайдера как конфиг, а не внешний watcher; auth-истёк → явная эскалация.
   - **Watchdog по состоянию turn**, а не по тишине вывода (урок #6748).
   - **HITL-lane** на `session/request_permission` (push), без polling.
   - **Human takeover**: документированный способ открыть сессию в TUI харнесса.
5. **Не переносить:** meta в env транспорта, activity-часы с вычитанием собственных нажатий,
   per-session nudge poller, матчеры текста экрана. Перенести идеи: exit code 75 как «capacity,
   не краш» (`runtime.go:76-80`), circuit breaker per endpoint, счёт rapid-crash, «3 неудачных
   resume → fresh».

## 5. Открытые вопросы и непроверенное

- **Не проводил экспериментов** (правило о квоте): реальное поведение claude-agent-acp / codex ACP /
  pi при исчерпании квоты, 429 и истёкшем auth — только по коду адаптера claude. Для codex-acp и pi
  не смотрел совсем. **[НП]**
- Доставляет ли `_claude/rateLimit` и session-failure extension обычному ACP-клиенту или только
  клиенту с capability «air» — не выяснено: в коде проверяется `clientSupportsAirCapability`. **[НП]**
- Допустимость биллинга подписки Claude/ChatGPT из стороннего оркестратора через ACP/SDK на
  2026-11 — условия не читал. **[НП]**
- Что делает `codex app-server-daemon` и годится ли он как готовый broker. **[НП]**
- Стоимость resume в токенах (prompt cache после рестарта процесса) не измерена. **[НП]**
- Данные по сбоям а)–ж) собраны делегированным обходом; лично перепроверены: паттерны `dialog.go`,
  константы quarantine, resume-флаги, таблица turn-семейств, отсутствие auth-матчеров,
  заголовки `work_ladder_*`, `provider_health_gate.go`, `crash_tracker.go`. Остальные `path:line`
  в столбцах с меткой **[П-С]** стоит перепроверить перед цитированием.
- Shallow-клон gastown (depth 50): мотивы выбора tmux в его документации не найдены; сравнение
  ограничено размером пакетов и наличием `internal/quota`, `internal/acp`.
- Разнесение строк на «артефакт/потребность» — экспертная оценка; погрешность ±30 %.

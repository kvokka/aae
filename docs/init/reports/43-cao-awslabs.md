# 43 — CAO (`awslabs/cli-agent-orchestrator`): изобретает ли владелец велосипед

Дата: 2026-11-10. Клон: `src/cli-agent-orchestrator` @ `f112f21` (main, 2026-11-10). Установлен ровно этот коммит
в `lab/cao/venv` (Python 3.13, uv, кэш в lab), сервер на `127.0.0.1:9889`, tmux только на lab-сокете (`TMUX_TMPDIR=lab/cao/tmux`),
`CAO_HOME_DIR=lab/cao/home`, `CODEX_HOME=lab/cao/codex-home`, агенты работали в `lab/cao/work/repo` (свежий `git init`).
Сырьё опытов: `lab/cao/out/`, `lab/cao/logs/`, лог сервера `lab/cao/home/logs/cao_*.log`.

Метки: **[П]** — ПРОВЕРЕНО (код, команда, лог); **[В]** — ВЫВОД; **[НП]** — НЕ ПРОВЕРЕНО. `S/` = `src/cli_agent_orchestrator/` в клоне.

## 1. Выводы за 60 секунд

- **Не велосипед: CAO решает другую задачу.** Это «LLM-супервизор раздаёт задачи LLM-воркерам через MCP-инструменты
  `handoff/assign/send_message`», где каждый агент — интерактивный TUI в окне tmux. Бидов, эпиков, ригов, графа формул,
  ордеров от трекера нет **[П]**.
- **По главной оси CAO — противоположность хотелкам.** Транспорт к агентам — только tmux (или экспериментальный herdr):
  ввод через `paste-buffer`, статус через regex по экрану. Это записано как принцип: «Headless `-p` and ACP modes are not
  CAO transports» (`docs/grok-cli.md:280`). Слой провайдеров весит 25,7 тыс. строк — тот же класс кода, что 20–26 тыс. строк
  tmux-рантайма Gas City, который владелец хочет выбросить **[П]**.
- **Опыт подтвердил цену скрейпа экрана — четыре «успешных провала»** **[П]**:
  1. После `kill -9` у claude CAO 35+ с показывает `completed`, принимает новую задачу (`success:true`) и вставляет её
     в zsh: `zsh: command not found: Reply`.
  2. Flow по cron упёрся в незнакомый диалог claude, вставил задачу в диалог и записал «Flow 'labflow' executed
     successfully». Задача потеряна.
  3. Реальный текст квоты claude («You've hit your session limit · resets 6:20am…») классификатор не распознаёт (`None`),
     и шаг засчитывается как ответ.
  4. После рестарта `cao-server` все живые агенты навсегда в `unknown`, письма им висят в `pending`.
- **tmux дал то, что обещал.** Агенты и их MCP пережили `kill -9` сервера, человек может сделать attach. Но сервер
  после рестарта не переподключает конвейер статусов, и выжившие агенты ему бесполезны (это признано в
  `docs/terminal-lifecycle.md`) **[П]**.
- **codex — да, через TUI; pi — нет.** Поддержан только форк `omp` (Oh My Pi); у codex смерть процесса ловится
  (`error` за 0,78 с), у claude — нет вовсе **[П]**.
- **Память хуже, а не лучше.** На каждого агента поднимается свой Python-процесс `cao-mcp-server` (131 МБ PSS) поверх
  claude (215 МБ PSS). Сервер — 128 МБ PSS, в простое ~0,45 % ядра **[П]**.
- **Общего MCP, кросс-харнессных хуков и слоёного конфига «харнесс + агент + риг» нет.** Есть серверные event-плагины
  на 5 событий, профили-markdown и per-session доставка MCP через флаги **[П]**.
- **Есть ценное для донорства** **[П]/[В]**:
  - durable-журнал Python-workflow с fingerprint шагов и replay;
  - гейт одобрения плана по хэшу;
  - генерация per-session `--mcp-config --strict-mcp-config` и `codex -c mcp_servers.*` без правки глобальных конфигов;
  - тройка `handoff/assign/send_message` с доставкой письма при idle.
- **Форк нецелесообразен** **[П]/[В]**. 153 тыс. строк Python плюс 361 тыс. строк тестов за 15 месяцев, ~200 коммитов
  за 4 недели, почти все в соавторстве Claude или Kiro Agent — тот же профиль роста, что у Gas City (H7). Чтобы
  довести CAO до хотелок, надо заменить ядро транспорта и дописать трекер, риги и граф. Это больше варианта A с нуля.
- **Неожиданное** **[П]**:
  - Личный конфиг оператора протекает в агентов CAO: хук deja, ответы по-русски.
  - Агент в каталоге без своего git изменил запись проекта `gascity` в `~/.claude.json` (§6).
  - CAO делает read-modify-write `~/.claude/settings.json` перед каждым стартом claude; у владельца ключ уже стоял,
    поэтому записи не было.

## 2. Что это и как устроено

**Проект** **[П]** (`gh api`, `git log`)

| Параметр | Значение |
|---|---|
| Язык, лицензия | Python, Apache-2.0 |
| Популярность | 1 539★, 318 форков |
| Возраст и история | создан 2025-07-29; 709 коммитов; 89 контрибьюторов (fanhongy 143, tedswinyar 86) |
| Релизы | v1.0.3 (2026-02-09) … v2.5.3 (2026-10-08); на PyPI 2.5.4 (колеса только x86_64/macOS) |
| Темп | с 14.10 — 196 коммитов: 183 трейлера `Co-Authored-By: Claude`, 92 `Kiro Agent`; 218 слитых PR за месяц |
| Открыто | 143 issues, 39 PR |
| Код | `S/` — 153 458 строк (services 62,6 тыс., providers 25,7 тыс., utils 11,7 тыс., api 11,5 тыс. — один `api/main.py` на 11 474 строки); тесты — 361 300; плюс `web/` (React) и `tui/` (Rust) |
| Публичные следы использования | анонс в блоге AWS Open Source (~окт. 2025), обзоры третьих лиц; списка продакшен-пользователей не нашёл **[НП]** |

**Процессы и транспорт** **[П]**

```
cao-server (FastAPI, 127.0.0.1:9889; SQLite $CAO_HOME_DIR/db; flock server.lock — один сервер на home)
└─ tmux: ДЕФОЛТНЫЙ сервер оператора (libtmux.Server() без сокета, S/clients/tmux.py:557, 680-692;
   глобально ставит exit-empty off и history-limit 50000)
   └─ сессия cao-<имя> → окно на агента → login-shell пользователя → в shell ВПЕЧАТЫВАЕТСЯ команда CLI
      ├─ claude --dangerously-skip-permissions --session-id … --model … --append-system-prompt-file …
      │        --mcp-config <id>.mcp.json --strict-mcp-config --disallowedTools … (S/providers/claude_code.py:662-865)
      │  └─ cao-mcp-server (stdio, свой Python-процесс на агента; S/mcp_server/server.py:3684-3688)
      └─ pipe-pane → fifo_writer.py → FIFO → StatusMonitor (regex по потоку + pyte-экран) → EventBus → inbox
```

- **Ввод.** `load-buffer`/`paste-buffer` плюс `Enter`; вывод — снимок `capture-pane` с эскалацией глубины
  (по разбору кода: `S/clients/tmux.py:1729-1900`, `2049-2067`).
- **Статусы.** `idle/processing/completed/waiting_user_answer/error/unknown` — regex по экрану. У claude `get_status`
  не проверяет возврат в shell (0 упоминаний `shell_baseline` в `claude_code.py`). У codex проверка есть
  (`S/providers/codex.py:2492-2498`).
- **Состояние.** SQLite: terminals, inbox, flows, workflow_run/run_step/run_event, plan_approval, handoff_results,
  memory/vault — 28 таблиц, перечислены read-only запросом.
- **Провайдеры.** `kiro_cli` (по умолчанию), `claude_code`, `codex`, `kimi_cli`, `copilot_cli`, `opencode_cli`, `hermes`,
  `cursor_cli`, `antigravity_cli`, `omp`, `grok_cli`, `mcode`, `mock_cli` (`S/models/provider.py:4-20`). Plain `pi` нет.
- **Модель работы.**
  - `handoff` синхронен. Создаёт окно воркера, ждёт `completed`, забирает текст, шлёт `/exit` и убивает окно.
  - `assign` асинхронен, воркер живёт дальше.
  - `send_message` кладёт письмо в inbox, оно доставляется при `idle/completed` получателя, reconcile раз в 30 с.
- **Flows.** Cron в SQLite, демон опрашивает раз в 60 с (`S/api/main.py:237-255`). Скрипт-гейт решает,
  запускать ли агента, и подставляет переменные.
- **Workflows.**
  - Python-скрипт через шим `cao_workflow.run_step()` (HTTP к серверу), журнал в SQLite, `resume` перезапускает скрипт
    и подставляет записанные результаты шагов.
  - Гейт одобрения плана включён по умолчанию (`S/services/approval_gate.py:1-12`).
  - YAML-уровень только последовательный; ретраи только у YAML (3 по умолчанию).
- **Хуки харнессов** не ставятся. Есть серверные event-плагины на 5 событий: `post_send_message`, `post_create_session`,
  `post_kill_session`, `post_create_terminal`, `post_kill_terminal` (`S/plugins/events.py:30-76`). Поле профиля
  `hooks` передаётся только в Kiro.
- **Профили** — markdown с frontmatter, тело = системный промпт. Поля: `provider`, `model`, `permissionMode`,
  `allowedTools`, `mcpServers`, `claudeConfig{effort, fallback_model}`, `codexConfig`, `codexProfile`, `native_agent`.
  Поля `env` нет, есть только `--env` при запуске. Слоя «риг/репо» нет.
- **Пульт человека.** `tmux attach`, web UI с PTY-WebSocket, `cao-ops-mcp-server`, Rust TUI, AG-UI. Из git собран
  только Python: `GET /` → 404, UI не проверял.
- **Worktree.** `use_worktree=True` на handoff/assign → `git worktree add <repo>/.cao/worktrees/<id>`
  (`S/services/worktree_service.py:6-10, 75`) **[П по коду, не запускал]**.
- **ACP.** Только issue #649 (open с 2026-08-21) «Expose CAO as an ACP server» — вверх, для внешних хостов. Вниз ACP
  отвергнут принципом.

## 3. Что показал запуск

Все строки — **[П]**. Модель claude — haiku, codex — `gpt-6-luna` через прокси из `lab/cao/codex-home`. Всего ~10 коротких
ходов LLM, одновременно не больше двух сессий харнесса.

| # | Опыт | Команда | Результат |
|---|---|---|---|
| E1 | воркер claude, «Reply with exactly: OK» | `cao launch --agents labworker --headless --auto-approve …` | **rc=1 за 31,7 с.** claude показал «Allow external CLAUDE.md file imports?»: lab лежит внутри `gascity`, чей `CLAUDE.md` импортирует `@AGENTS.md`. CAO этот диалог не знает → `waiting_user_answer`, `initial_delivery.state=failed`. После `POST /terminals/{id}/key?key=Enter` (по умолчанию «No») и `POST …/input`: `processing` через 0,8 с, `completed` через 4,55 с, `output?mode=last` → `"OK"` |
| E4 | `kill -9` процесса claude у E1 | `kill -9 3599216` + поллер `scripts/poll.py` | статус 35+ с остаётся `completed`. `POST /input` → `{"success":true}`, на экране `zsh: command not found: Reply`, `output` по-прежнему отдаёт старое `"OK"`. В логе сервера ни строки об этом |
| E2 | handoff супервизор → воркер | `cao launch --agents labsup … "Use … handoff … labworker …"` | `HANDOFF=OK` за 32,4 с с двумя холодными стартами. По логу: окно воркера создано через 7,2 с после промпта, задача доставлена ещё через 7,2 с (2,7 с — инициализация zsh владельца, ~1,2 с — старт claude, остальное — ожидания «settle»); затем `/exit` и kill окна |
| E3 | `assign` + `send_message` обратно | input супервизору | `assign` вернулся за ~7 с. Письмо `ASSIGN-DONE` доставлено, когда супервизор стал `completed`; ответ `GOT=ASSIGN-DONE` на 22,4 с. Воркер остался жив. Супервизор ответил по-русски, в пане `SessionStart:startup says: deja loaded…` — личный конфиг оператора в агенте |
| E5 | `kill -9` самого `cao-server`, потом старт | `kill -9 <pid>`; `cao-server` | агенты, их `cao-mcp-server` и окна живы. После старта оба терминала `unknown`, поколение 0. Задача выжившему воркеру дошла и `output` = `"OK2"` (снимок экрана), но статус так и остался `unknown`. Письмо супервизору висело `pending` > 45 с (дольше reconcile 30 с), потом стало `failed` при удалении сессии |
| E6 | воркер codex | `cao launch --agents labcodex …` | `• OK` за 12,6 с с холодным стартом. Маркер TUI `•` попал в результат. `~/.codex/config.toml` не тронут (sha256 до и после одинаков), rollout лёг в lab `CODEX_HOME` |
| E7 | `kill -9` codex | `kill -9 3624650` (PID проверен: zsh → lab-tmux, `CODEX_HOME` = lab) | `completed` → `error` за 0,78 с. `POST /input` → «provider is in ERROR state … Refusing to deliver input». Перезапуска нет |
| E8 | flow каждую минуту со скрипт-гейтом | `cao schedule add lab/cao/flows/labflow.md` | сработал в 23:46:12 в cwd сервера (`lab/cao/work`, не в репо) и упёрся в тот же диалог. Через 60 с (`provider_init_timeout`) CAO вставил 27 символов задачи в диалог, `Enter` выбрал «No», claude стартовал с пустым промптом, CAO показывает `processing`, в логе «Flow 'labflow' executed successfully». Задача потеряна |
| E9 | Python-workflow: шаг claude → шаг codex | `cao workflow create/run labwf` | первый запуск — `403 approval_required` с `plan_id`. Я одобрил свой лабораторный план (`cao workflow approve`). Прогон 32,8 с, `completed`; в `workflow_run_step.result_json`: a=`"ALPHA"`, b=`"• BETA-ALPHA"`. `GET …/events` пуст, `output_json` у шагов `null` |
| E10 | классификатор ошибок офлайн | `python -I scripts/classify.py` (функция установленного пакета) | `API Error: 429 {…}` → `provider_error`; `API Error: 529 {…}` → `provider_error`. Реальный текст квоты claude (из транскрипта, отчёт 14) → **`None`**. Мои приближения текстов codex usage-limit, «exceeded retry limit, last status: 429», `Invalid API key`, `Prompt is too long` → `None` **[В: тексты не сняты вживую]** |

**Память и CPU** **[П]** (`/proc/*/smaps_rollup`):

| Процесс | PSS |
|---|---|
| `cao-server` | 128 МБ, 12 потоков; простой 9 тиков CPU за 20 с ≈ 0,45 % ядра |
| tmux-сервер | 3 МБ |
| claude-агент | 215 МБ |
| его `cao-mcp-server` | 131 МБ |
| FIFO-писатель | 7,5 МБ |
| zsh | 4,7 МБ |
| codex | 200 МБ RSS |
| его `cao-mcp-server` | 140 МБ RSS |

Накладные расходы CAO на агента — около +140 МБ, то есть +65 % к самому claude.

## 4. Сравнительная таблица

«GC» — Gas City, как его использует владелец (отчёты 02, 03, 08); «Хотелка» — §6 FINAL-REPORT.
Вердикт по CAO: есть / частично / нет.

### 4.1 14 must-требований (отчёт 02 §4)

| # | Требование | CAO | Доказательство | GC |
|---|---|---|---|---|
| 1 | риги, эпик над несколькими ригами | **нет** | нет понятия рига; только `working_directory` сессии | есть, эпик через шимы |
| 2 | бид-стор: типы, `blocks`, KV, `ready` | **нет** | SQLite только для сессий, inbox, прогонов; grep `bead/epic/depends_on` пуст | есть (bd + Dolt) |
| 3 | роль = харнесс + модель + effort + env + права; fresh/resume; pool | **частично** | `provider/model/claudeConfig.effort/permissionMode/allowedTools`; env только `--env`; resume только `--resume-session-id`; pool нет | есть |
| 4 | скриптовые агенты без LLM | **частично** | скрипт-гейт flow, Python-workflow; exec-шага как агента нет | есть |
| 5 | claim-протокол, возврат при смерти | **нет** | push через handoff/assign; при смерти claude работа не возвращается (E4) | есть |
| 6 | маршрут на роль, deps, условие, check-петля, fan-out, scope | **частично** | всё — произвольным Python в workflow с журналом (E9); декларативного графа нет, YAML последовательный | есть (v2) |
| 7 | гейты human и внешнего условия с таймаутом | **частично** | одобрение плана целиком (`approval_gate.py`), гейт flow; гейта-шага нет | есть |
| 8 | worktree на задачу | **есть** | `use_worktree` (`worktree_service.py`) **[П по коду]** | есть |
| 9 | pack-расширения, vars рига | **частично** | профили, skills, agent-plugins; vars рига нет | есть |
| 10 | хуки на роль → claude/codex/pi | **нет** | хуки харнессов не ставит; event-плагины только серверные | нет |
| 11 | общий MCP на город | **нет** | `cao-mcp-server` stdio на агента (ps, E1) | нет |
| 12 | liveness по протоколу, не по экрану | **нет** | regex экрана; смерть claude не видна (E4) | нет (экран) |
| 13 | событийный хоп ≤ 5 с | **частично** | inbox по событию статуса; хоп handoff ≈ 7,2 с от создания окна до доставки задачи (E2); внутри опросы по 1 с | p50 15 с |
| 14 | attach оператора, почта с dedup | **есть / частично** | `tmux attach`, PTY-WebSocket, inbox; dedup не видел **[НП]** | есть (tmux) |

Итог по CAO: полностью 2 из 14, частично 7, нет 5. Нет именно ядра хотелок — трекера, ригов, claim и liveness.
У Gas City все 14 есть хотя бы в виде шимов **[В]**.

### 4.2 Гипотезы H1–H11 глазами CAO

| H | Что показывает CAO |
|---|---|
| H1 tmux не нужен | CAO — живой пример цены tmux как канала управления (E1, E4, E5, E8) и его пользы как хоста процессов (E5). Подтверждает вердикт FINAL-REPORT |
| H2/H3 ACP | ACP вниз отвергнут принципом (`docs/grok-cli.md:280`); вверх — только issue #649. pi не поддержан |
| H4 память | хуже Gas City: на агента +1 Python-MCP-процесс (131 МБ PSS) |
| H5 br/bv | трекера нет вовсе |
| H6 молекулы/конвои | нет ни молекул, ни конвоев, но нет и «эпик + дети». Граф — Python-скрипт: подтверждает третий путь «формула = скрипт + агенты» |
| H7 раздутость | 153 тыс. строк за 15 месяцев, почти всё в соавторстве агентов: тот же паттерн, что у Gas City |
| H8 тики | событийная шина внутри, но статусы — скрейп, опросы 0,5–4 с, flows раз в 60 с |
| H9 MCP/хуки/конфиг | ничего из трёх |
| H10 пульт | сильная сторона: attach и web-PTY к живому TUI, ops-MCP. Не ACP |
| H11 переписывать | CAO не закрывает потребность: готовой альтернативы по-прежнему нет |

### 4.3 18 классов сбоев (отчёт 34), сжато

| Классы | CAO |
|---|---|
| 1 квота | **нет**: реальный текст claude → `None`, шаг «успешен» (E10) |
| 2 rate limit | **частично**: первая строка `API Error: 429` только в шагах workflow/handoff; backoff и «светофора» на аккаунт нет |
| 3 overload | **частично**: `API Error: 529` да, «Repeated 529» нет |
| 4–5 auth, billing | **нет** для claude/codex (шаблоны только у grok/mcode) |
| 6–7 overflow, config | **нет** / **частично** (model not found) |
| 8 краш харнесса | claude **нет** (E4), codex обнаруживает (E7); рестарта нет |
| 10 смерть оркестратора | **частично**: агенты живы, наблюдение потеряно (E5) |
| 11–12 stall, runaway | **нет** / **частично** (таймауты handoff и шага 600 с) |
| 13 ждёт человека | **частично**: regex известных диалогов; незнакомый диалог ломает старт (E1, E8) |
| 15–17 MCP, транскрипт, workspace | **нет**; человек и CAO пишут в одну панель без lease |
| 18 hollow done | **нет**: E8 и E10, плюс вставка в shell (E4) |

**Новые ловушки «успешного провала», которых нет в отчёте 34** **[П]**:
- задача вставлена в shell после смерти claude;
- flow «executed successfully» с потерянной задачей;
- после рестарта `unknown` трактуется как idle (`docs/terminal-lifecycle.md`, «After a server restart»);
- маркер TUI в результате (`• OK`).

### 4.4 Остальные оси

| Ось | CAO | GC | Хотелка |
|---|---|---|---|
| Процессы на агента | shell + CLI + Python-MCP + FIFO-писатель | shell + CLI + stdio-MCP | драйвер в демоне, общий MCP |
| Конфиг | профиль-markdown + `settings.json` + env; личный `~/.claude` оператора не отсекается | слои флагов, overlay | харнесс → агент → риг, `settingSources=[]` |
| Формулы/ордеры | workflows (Python, журнал), flows (cron + скрипт-гейт) | formula v2, orders | формулы без молекул, durable run |
| Глобальные побочные эффекты | RMW `~/.claude/settings.json`; глобальные опции дефолтного tmux-сервера | свой `-L gc` | никаких |

## 5. Велосипед или нет, и что взять

**Вердикт [В]: не велосипед.**
- CAO — оркестратор диалогов «супервизор ↔ воркеры» поверх TUI.
- Ценность Gas City для владельца иная: работа, переживающая сессии в бидах; риги; граф формул и ордеры; событийный
  контроллер. Её CAO не даёт.
- По транспорту CAO стоит там, откуда владелец уходит, и делает это сознательно.

**Как база — нет.**
- Пришлось бы заменить ядро: провайдеры (25,7 тыс.), backends/clients (~9 тыс.), status monitor и FIFO.
- Пришлось бы дописать трекер br, риги, граф, общий MCP, хуки, claim и liveness.
- При этом тащить ~100 тыс. строк ненужного: memory, vault, wiki, decisions, Kiro, AG-UI.
- Оценка: больше 20–32 тыс. строк варианта A, плюс дрейф апстрима ~50 коммитов в неделю **[В]**.

**Как слой сессий под диспетчер C3 — нет.**
- C3 нужны структурные события (`claude -p` stream-json, codex app-server), а CAO даёт regex-статусы.
- В 3 из 4 «kill»-опытов CAO ведёт себя хуже, чем «убить и resume» из отчёта 34.
- Единственная ниша — флот интерактивных ролей (mayor, desk) с attach. Её Gas City уже закрывает тем же tmux.

**Как донор идей — да, недорого.**
1. Per-session доставка конфига без правки глобальных файлов: `--mcp-config <file> --strict-mcp-config`
   `--disallowedTools`, `--append-system-prompt-file` (`claude_code.py:662-865`) и `codex -c mcp_servers.*`
   (`codex.py:1811-1896`, по разбору кода). Ровно механизм §6.5 FINAL-REPORT.
2. Список встроенных инструментов Claude, которые надо отключать агентам оркестратора: `SendMessage`, `ListAgents`,
   `Cron*`, `Workflow`, `EnterWorktree`, `Team*`. Урок: агент ответил через родной `SendMessage`, и супервизор ждал
   впустую (`docs/claude-code.md`, Tool Restrictions).
3. Тройка `handoff` (синхронно, одноразовый воркер), `assign` (асинхронно), `send_message` (inbox, доставка при idle)
   как готовая модель «роль → mayor» почты (must 14).
4. Durable Python-workflow: журнал шагов с call fingerprint, replay при `resume`, запрет недетерминизма на верхнем
   уровне, `step(recovery=…)`. Сильный прототип варианта (б) «durable run» и третьего пути
   «формула = скрипт + агенты».
5. Гейт одобрения по хэшу плана — дешёвая защита от того, что агент сам запустит изменённую формулу.
6. Один сервер на home через flock и всё состояние под `CAO_HOME_DIR`.
7. herdr-бэкенд с нативными событиями статуса как доказательство тезиса «статус — из рантайма, не из regex»
   (`docs/herdr.md:5`).

**Чего не брать [П]:**
- дефолтный tmux-сервер оператора и глобальные опции на нём;
- RMW `~/.claude/settings.json`;
- автонажатия в диалогах;
- `unknown ≈ idle`;
- извлечение ответа с экрана;
- классификатор по первой строке.

**tmux — честно.**
- **Польза** **[П]**: CAO показал ровно две полезные вещи tmux — процессы пережили `kill -9` оркестратора (E5),
  и человек может войти в живую панель.
- **Вред** **[П]**: всё остальное — от использования tmux как канала управления:
  - смерть claude невидима, ввод уходит в shell (E4);
  - незнакомый диалог ломает старт и тихо съедает задачу (E1, E8);
  - мусор в результате (E6, E9);
  - после рестарта сервер слеп (E5). Это вина CAO, а не tmux: FIFO не переподключается
    (`S/services/terminal_service.py:3112` — только при создании терминала).
- **Насколько это противоречит хотелкам** **[В]**: полностью по транспорту, не по пульту.
- **Насколько это проблема для владельца** **[В]**:
  - для его конвейера с бидами и графом — блокирующая: H1 и must 12 противоречат архитектуре CAO;
  - для интерактивного флота «как Gas Town» — нет: это та же ниша, что у Gas City, без бидов.

## 6. Побочные эффекты на машине

| # | Что | Где | Что сделать (решает владелец) |
|---|---|---|---|
| 1 | Запись проекта `/home/ubuntu/proj/work/gascity` изменена claude-агентом из flow (E8): cwd `lab/cao/work` не git-репо, claude отнёс его к корню git `gascity`. `hasClaudeMdExternalIncludesWarningShown`: `false → true`, `lastSessionId`: `124a92d4-6926-4c9b-9fca-b0847f7d9d70 → e1e8cba7-63f1-4d14-b54a-6197a8502581`; `Approved` осталось `false`. Сверено по `~/.claude/backups/.claude.json.backup.1794329243238` (до) и `…1794329304055` (после) | `~/.claude.json` | вернуть два ключа вручную, если важно: иначе для подкаталогов `gascity` диалог о внешних импортах больше не появится, импорты так и останутся выключены |
| 2 | Новая запись проекта `lab/cao/work/repo` (trust принят, импорты отклонены) | `~/.claude.json` | удалить при желании |
| 3 | Транскрипты лабораторных сессий claude (5 файлов) | `~/.claude/projects/-home-ubuntu-proj-work-gascity-tmp-gc-alt-research-lab-cao-work-repo` | удалить при желании |
| 4 | `~/.claude/settings.json` и `~/.codex/config.toml` не изменены: sha256 `308a0135…` и `006a673f…` до и после. CAO пропустил запись, потому что `skipDangerousModePermissionPrompt` уже стоял (`claude_code.py:896-897`) | — | при использовании CAO на чистой машине он сам допишет ключ |
| 5 | Плагин deja владельца создал `.deja/` в lab-репо; `gascity/.deja` не тронут (mtime 18:30) | `lab/cao/work/repo/.deja` | — |
| 6 | Глобальные pre-commit хуки git владельца отработали на `git init`-коммите lab-репо | — | — |
| 7 | Лабораторные файлы: venv 203 МБ, uv-cache 290 МБ, codex-home 98 МБ, home 1,2 МБ; устаревший сокет `lab/cao/tmux/tmux-1000/default` | `lab/cao/` | удалить каталог при желании |
| 8 | Ходы LLM: ~8 у claude haiku, 2 у codex через прокси; одобрен собственный лабораторный план workflow (CAO называет это «human act») | — | — |
| 9 | Процессы: `cao-server` остановлен по PID; lab-tmux — `tmux -S lab/cao/tmux/tmux-1000/default kill-server`; `pgrep -af gc-alt-research/lab/cao` пуст. tmux `-L gc`, claude владельца (PID 1309983, 1310029), dolt, supervisor не трогались | — | — |

## 7. Не проверено

- Web UI и PTY-WebSocket (из git фронтенд не собран, `GET /` → 404), Rust TUI, `cao-ops-mcp-server`, AG-UI.
- herdr-бэкенд: herdr установлен, но запуск рисковал задеть herdr владельца.
- Реальные квота и 429 вживую. Тексты ошибок codex в E10 — мои приближения.
- Workflow с параллельным fan-out, `resume` после падения, worktree-режим, эфемерные агенты, memory и self-learning.
- `--resume-session-id` после краша claude; долгие ходы; компакция; десятки агентов.
- Провайдер `omp` (Oh My Pi) вживую.
- Dedup писем inbox; поведение `assign` при смерти воркера.
- Кто использует CAO в продакшене — нашёл только анонс AWS и обзоры.

Источники вне репозитория:
- https://github.com/awslabs/cli-agent-orchestrator
- https://aws.amazon.com/blogs/opensource/introducing-cli-agent-orchestrator-transforming-developer-cli-tools-into-a-multi-agent-powerhouse/
- https://awslabs.github.io/cli-agent-orchestrator/
- https://hysenlabs.com/projects/awslabs-cli-agent-orchestrator
- issues #649, #152 через `gh api`

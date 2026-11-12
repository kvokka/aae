# 36. Опыты второй волны: общий HTTP MCP на город, `claude --bg` как супервизор, два клиента на одном codex app-server

Дата: 2026-11-10. Лаборатория: `lab/wave2-exp/` (все пути ниже относительно `tmp/gc-alt-research/lab/wave2-exp/`, если не указано иное).
Исполнитель №36 упал на HTTP 429, не успев написать отчёт. Этот отчёт собран преемником по сохранённым логам и по транскрипту исполнителя (`~/.claude/projects/-home-ubuntu-proj-work-gascity/e4205c8a-…/subagents/agent-ac91c580b3f847828.jsonl`; выжимки его команд и их вывода сохранены в `logs/prev-executor-commands.txt` и `logs/prev-executor-results.txt`). **Новых прогонов не было.** Время в логах указано в UTC (03:4x Z = 10:4x по +07).
Модели: claude — `haiku` (`claude-haiku-5-5`); codex — `gpt-6-luna`, effort low, через прокси владельца (`codex-home/config.toml`, провайдер `myproxy`); pi — `google/gemini-3.5-flash-lite`. Фейковый провайдер не использовался. По логам вышло около 26 коротких LLM-ходов [ВЫВОД].

## 1. Выводы за 60 секунд

- **[ПРОВЕРЕНО] Один процесс MCP обслужил все три харнесса.** Сервер `city-mcp` (pid 1199351) принял 6 вызовов `whoami` от двух сессий claude (через ACP), двух тредов codex (один app-server) и двух процессов pi. Каждый вызов пришёл с правильной идентичностью: `perAgent: {"claude-1@rig-a":1,"claude-2@rig-b":1,"codex-1@rig-a":1,"codex-2@rig-b":1,"pi-1@rig-a":1,"pi-2@rig-b":1}` (`logs/exp1-run.out:34`).
- **[ПРОВЕРЕНО] Ни один харнесс не запустил ни одного процесса MCP.** В деревьях процессов нет MCP-детей (`logs/exp1-run.out:11-14,22-24,29-32`).
- **[ПРОВЕРЕНО] Версии протокола расходятся.** Claude Code 2.1.321 сначала пробует stateless 2026-07-28 (`server/discover`), а если сервер его не знает, откатывается на `initialize` 2025-11-25. codex 0.166.0 говорит только на 2025-06-18, pi 1.5.0 — только на 2025-11-25. Сервер, который понимает только 2026-07-28, codex и pi отвергают с `-32022` (`logs/exp1b-C.out`). Значит, общий сервер города обязан понимать обе эпохи протокола.
- **[ПРОВЕРЕНО] `Mcp-Session-Id` и постоянный GET-поток SSE не обязательны.** Все три клиента работают и со stateful-сервером, и со stateless-сервером без session id, и при ответе `405` на GET.
- **[ПРОВЕРЕНО] Сервер перезапустили на том же порту.** codex и pi переподключаются лениво и незаметно: первый же вызов получает `404`, клиент делает `initialize` заново и повторяет вызов за ≈5 мс. Claude переподключается фоново с нарастающей задержкой: от 6.8 до 20.6 с после подъёма сервера, а в одном прогоне за 25 с так и не успел, и ход сообщил «MCP server "city" failed to start». Если сервер говорит на 2026-07-28 stateless, перезапуск для claude прозрачен: следующий `tools/call` прошёл без рукопожатия.
- **[ПРОВЕРЕНО] Память.** Общий сервер на Node занимает 74.5 MB RSS и 38.6 MB PSS. Один stdio-экземпляр минимального MCP на Node — 58–62 MB RSS и 23–26 MB PSS. Общий сервер выгоднее уже со второй сессии [ВЫВОД].
- **[ПРОВЕРЕНО] `claude --bg` запускается без интерактива, но только в доверенном каталоге.** В свежем git-репо ответ был `Workspace not trusted … accept the trust prompt`. В подкаталоге доверенного репо gascity запуск прошёл.
- **[ПРОВЕРЕНО] Второе сообщение через сокет `/tmp/cc-socks/<pid>.sock` доходит, но без подтверждения.** Сокет ничего не отвечает. Сообщение попадает в сессию как сообщение от другой сессии («Another Claude session sent a message… not typed by your user»), а не как ввод пользователя.
- **[ПРОВЕРЕНО] `kill -9` обрабатывается непоследовательно.** Убитую простаивающую сессию супервизор не перезапустил: `state: done`, `pid` нет. Сессию, убитую посреди хода, супервизор перезапустил примерно через 10 с, но прерванный ход не продолжил, и введённое сообщение потерялось. После этого `agents --json` показывает `idle`+`working`, а `state.json` — `blocked`: состояние противоречиво.
- **[ВЫВОД] `claude --bg` с демоном не годится как основной транспорт и супервизор оркестратора.** Нет потока событий (`claude logs` — снимок экрана TTY), нет подтверждения доставки, состояние — только опросом; демон один на пользователя и общий с его сессиями. Годится как интерфейс для человека.
- **[ПРОВЕРЕНО] Codex: два `turn/start` в один тред не ставятся в очередь и не отвергаются, а сливаются в один ход.** Оба ответа несут один и тот же turn id, в ходе оказываются оба `userMessage`, а модель ответила только на последнее сообщение (`A-DONE` потерян). Это так и для хода, который уже идёт, и для одновременного старта на простаивающем треде.
- **[ПРОВЕРЕНО] Codex: запрос на подтверждение (approval) приходит всем подписанным клиентам с одним и тем же JSON-RPC id, и выигрывает первый ответ.** Поздний ответ молча игнорируется; `serverRequest/resolved` рассылается всем.
- **[ПРОВЕРЕНО] Codex: источник ввода виден только по конвенции.** Параметр `turn/start.clientUserMessageId` возвращается как `item.clientId`, его видят оба клиента, он сохраняется в rollout. `turnTrigger` в событиях не появляется. Настоящая очередь есть, это `codex queue … --remote unix://SOCK`, то есть `thread/queue/changed`, после которого начинается новый ход.

## 2. Опыт 1 — общий Streamable HTTP MCP на город (H9)

### Что запускалось

- Сервер `scripts/city-mcp.mjs` — свой, на Node, без зависимостей, один инструмент `whoami`. Он возвращает счётчик вызовов, pid, boot-id, заголовки `X-City-Agent`/`X-City-Rig`, `Mcp-Session-Id` и `MCP-Protocol-Version`. Режимы `MCP_MODE`: `stateful`, `stateless`, `dual` (2026-07-28 stateless плюс прежний протокол), `modern` (только 2026-07-28). Ответ на GET регулирует `MCP_GET_SSE`. Режимы `modern`/`dual` — лабораторное приближение спецификации из `spec/*.mdx`. Порт 46507, адрес 127.0.0.1. Журнал запросов: `logs/mcp-server.jsonl`, сводка — `logs/mcp-server-summary.txt`.
- Драйвер `scripts/exp1.mjs` (`dry` — без LLM, `run` — с LLM):
  - claude: ACP-адаптер `@agentclientprotocol/claude-agent-acp` 0.102.0 из `lab/claude`, `session/new.mcpServers=[{type:"http",url,headers}]`, `_meta.claudeCode.options={settingSources:[],strictMcpConfig:true}`;
  - codex: собственный `codex app-server` (stdio) с `CODEX_HOME=codex-home`, `thread/start.config.mcp_servers.city={url,http_headers}`;
  - pi: `pi --mode rpc` с `PI_CODING_AGENT_DIR=pi-agent`, в `pi-agent/mcp.json` заданы заголовки `"${CITY_AGENT}"`/`"${CITY_RIG}"`.
- `scripts/exp1b.mjs`. Фаза C — матрица совместимости без LLM по четырём режимам сервера (логи: `logs/exp1b-C.out`). Фаза D — переподключение claude и переход на сервер обеих эпох (dual-era) (`logs/exp1b-D.out`).
- RSS stdio-серверов для сравнения: по 4 экземпляра `lab/claude/mcp-min.mjs` (Node) и `lab/codex/scripts/mini_mcp.py` (Python) → `logs/stdio-rss.txt`.

### Наблюдения

**Один процесс, идентичность по заголовкам** [ПРОВЕРЕНО]. Все шесть ответов содержат `server_pid=1199351 boot=945e8ed0`, например:
`whoami call=4 server_pid=1199351 … agent=codex-1 rig=rig-a mcp_session=135ef4f0 proto_header=2025-06-18` (`logs/exp1-run.out:20`);
`whoami call=6 … agent=pi-1 rig=rig-a … proto_header=2025-11-25` (`:27`).
Заголовки с `${VAR}` в pi подставились из окружения процесса. MCP-сессия у каждой сессии или треда своя (у двух тредов одного app-server разные `Mcp-Session-Id`).

**Процессы харнессов** [ПРОВЕРЕНО] (`logs/exp1-run.out:11-32`):

| Харнесс | Дерево при 2 сессиях | MCP-детей |
|---|---|---|
| claude через ACP | `node index.js` 151 MB → 2× `claude --output-format stream-json …` по 229 MB RSS | 0 |
| codex app-server | один `codex app-server` 181 MB (+`codex-code-mode-host` 19 MB, не MCP) | 0 |
| pi rpc | два отдельных `pi --mode rpc`, 100/97 MB | 0 |

**Рукопожатие и версии** [ПРОВЕРЕНО] (`logs/mcp-server-summary.txt`):
- claude (`ua=claude-code/2.1.321 (sdk-ts…)`) начинает с `POST server/discover`, `MCP-Protocol-Version: 2026-07-28`. Stateful-сервер отвечает 400 «missing Mcp-Session-Id», stateless-сервер — `-32601`; после этого claude отправляет `initialize req=2025-11-25`. На dual-сервере `server/discover` получает 200, и дальше идут `tools/list`/`tools/call` без `initialize` и без session id (`era=modern`).
- codex (`codex-mcp-client/0.166.0`) шлёт `initialize req=2025-06-18`, затем GET SSE с `Accept: text/event-stream, application/json`.
- pi (`Bun/1.4.4`, clientInfo `pi 1.5.0`) шлёт `initialize req=2025-11-25`, затем GET SSE.

**Матрица совместимости** (`logs/exp1b-C.out`) [ПРОВЕРЕНО]:

| Режим сервера | claude | codex | pi |
|---|---|---|---|
| stateful, GET→405 | connected | ok | connected |
| stateless (без session id), GET→405 | connected (через откат на `initialize`) | ok, `mcp_session=none` | connected |
| dual (2026-07-28 + прежний) | connected, stateless 2026-07-28 | ok, прежний протокол | connected, прежний протокол |
| только 2026-07-28 | connected | `-32022 … requested 2025-06-18` | `MCP HTTP request failed with status 400 … requested 2025-11-25` |

**Прочие особенности** [ПРОВЕРЕНО]:
- У claude MCP-инструмент сначала ищется через `ToolSearch` (инструменты отложенные), затем вызывается. В режиме `default` он требует подтверждения: `session/request_permission` для `mcp__city__whoami` с вариантами `allow_once`/`allow_always`/`reject_once` (`logs/exp1-run.out:8-10`).
- `mcpServerStatus/list` у codex открывает отдельную временную MCP-сессию и делает GET с `MCP-Protocol-Version: 2024-11-05`. Если сервер отвечает на этот GET потоком SSE, вызов статуса занимает ≈5 с (03:46:50.195 → 03:46:55.188 в сводке).

**Убийство и перезапуск сервера на том же порту** [ПРОВЕРЕНО]:
- Пока сервер лежит, вызов возвращает ошибку внутри хода, сам ход завершается нормально. claude: `FAILED: MCP server "city" is not connected`. codex: `Transport send error … error sending request for url`. pi: `ECONNREFUSED` (`logs/exp1-run.out:37-39`).
- codex и pi: после перезапуска первый `tools/call` со старым session id получает `404 unknown session`, сразу следует `initialize` и повторный вызов, который проходит (03:48:29.300→.306 у codex, 03:48:27.062→.065 у pi). Это видно и в ответах: `UP codex-1 -> … call=2 server_pid=1219423` (`logs/exp1-run.out:43-44`).
- claude переподключается фоново, без участия ходов:

| Прогон | Простой сервера | Повторный `initialize` от claude |
|---|---|---|
| dry, вызовов во время простоя не было | 9 с | через 6.8 с после подъёма |
| D1, вызовов во время простоя не было | 20 с | через 15.7 с после подъёма (`yes, +36.2s after kill`) |
| D2, неудачный вызов во время простоя | 9 с | через 20.6 с, следующий ход прошёл |
| run, неудачный вызов во время простоя | 20 с | только в момент следующего промпта (через 25.7 с); ход ответил `MCP server "city" failed to start: Its tools are unavailable in this session` (`logs/exp1-run.out:42`) |

  Принудительно переподключить можно промптом `/mcp reconnect city` через ACP: ответ `- Reconnected city` (`logs/exp1b-D.out`).
- На dual-сервере claude говорит stateless. После перезапуска сервера (D3) следующий ход сразу выполнил `tools/call` на новом pid 1257349 без рукопожатия: `… mcp_session=none proto_header=2026-07-28 era=modern-stateless` (`logs/exp1b-D.out`, последние строки).

**Память** [ПРОВЕРЕНО]: общий сервер — `rss=74480kB pss=38638kB`. stdio на Node — 58.5–61.8 MB RSS и 23.2–26.4 MB PSS на экземпляр. stdio на Python — 17.5–18.7 MB RSS и 8.6–9.7 MB PSS. Строки по ~4 MB в `logs/stdio-rss.txt` — это, по всей видимости, обёрточные оболочки, которые захватил `pgrep -f` [ВЫВОД].

### Вывод для дизайна

- Общий MCP на город технически готов у всех трёх харнессов уже сейчас [ПРОВЕРЕНО]. Идентичность агента и рига надёжно передаётся HTTP-заголовками, которые задаются на сессию или тред. У claude их задаёт ACP `mcpServers[].headers`, у codex — `thread/start.config.mcp_servers.*.http_headers`, у pi — `mcp.json` с `${VAR}` и env на процесс.
- Сервер (или шлюз) города должен понимать обе эпохи протокола: прежнюю 2025-06-18/2025-11-25 и 2026-07-28 stateless. GET SSE можно не поддерживать и отвечать 405. Лучше stateless без session id: тогда перезапуск сервера не порождает 404 и повторные рукопожатия [ВЫВОД; прозрачность stateless-прежнего протокола для codex/pi при перезапуске не проверялась].
- Оркестратор должен учитывать задержку переподключения claude. Варианты: перед ходом отправлять `/mcp reconnect <name>` через ACP; перезапускать MCP только между ходами; давать claude 2026-07-28 [ВЫВОД].
- Экономия памяти растёт линейно с числом сессий. При 6 сессиях и 1 MCP на Node это ≈150 MB PSS у stdio против ≈39 MB у общего сервера [ВЫВОД, арифметика по измерениям]. Реальные MCP (браузерные и т. п.) тяжелее, так что экономия будет больше.

### Что не удалось

Codex через `codex-acp` (`mcpServers`) и pi через ACP-адаптер не проверялись. Прозрачность перезапуска stateless-сервера прежней эпохи для codex и pi не проверялась. Сервер самописный: поведение SDK-серверов при `server/discover` от claude может отличаться [НЕ ПРОВЕРЕНО].

## 3. Опыт 2 — `claude --bg` + демон как готовый супервизор

### Что запускалось

Документация: code.claude.com/docs/en/agent-view и /cross-session-messaging (выжимки — в `logs/prev-executor-results.txt`, блоки 03:52:57 и 03:52:59). Строки из бинарника — в `logs/exp2-binary-strings.txt`.
Все запуски шли через `scripts/cenv.sh`, который очищает окружение от переменных `CLAUDE*` вызывающей сессии. Бинарник явно указан как 2.1.321, под версию демона. Команда запуска:
`claude --bg --model haiku --setting-sources "" --strict-mcp-config --tools Bash --allowedTools "Bash(sleep:*)" --name w2lab-bg1 "Reply with exactly: BG-OK-1" < /dev/null`.
**Все фоновые сессии жили в общем демоне Claude владельца** (`claude daemon run --origin transient`, PID 952951; его родитель — интерактивная сессия владельца, PID 77108). Демон и его spare-процессы не трогались, чужим сессиям ничего не посылалось.

### Наблюдения

- **Доверие к каталогу** [ПРОВЕРЕНО]. В `work/bg` (там сделали `git init`) вышло `rc=1`: `Workspace not trusted. Run claude in …/work/bg once and accept the trust prompt, then retry.` (`logs/exp2-bg-start.err`). В `work/bg2` git toplevel — это `/home/ubuntu/proj/work/gascity`, и там запуск прошёл с `rc=0`, `backgrounded · 124a92d4 · w2lab-bg1` (`logs/exp2-bg-start2.out`). Доверие наследуется от доверенного корня репозитория [ВЫВОД]. Диалог доверия не принимался. Документация: `--bg` вместе с `-p` отклоняется.
- **Запуск и время** [ПРОВЕРЕНО]. Сессия не создаётся с нуля: демон забирает заранее прогретый spare-процесс. В `daemon.log` записано `bg claimed-spare 124a92d4 (shell)` и `bg spare spawned host pid=1295425`, процесс 953076 — это `claude bg-spare …claim.sock`, запущенный в 10:25:40. От старта до `done` прошло ≈4.4 с (`createdAt 03:57:36.05`, `firstTerminalAt 03:57:40.47`). CLI сразу возвращает управление, то есть смерть запустившего процесса сессии не касается.
- **`claude agents --json`** [ПРОВЕРЕНО] возвращает поля `pid, id, cwd, kind:"background", startedAt, sessionId, name, status (busy|idle), state (working|done|blocked|stopped)` (`logs/exp2-agents-poll.json`). Переход `busy/working` → `idle/done` занял ≈3 с (`logs/exp2-poll1.out`). Один вызов длится 0.11–0.14 с. В списке видны и сессии города и владельца (`logs/exp2-agents-before.json`).
- **Вывод** [ПРОВЕРЕНО]. `claude logs 124a92d4` отдаёт снимок экрана TUI с ANSI-кодами (`logs/exp2-logs1.out`, 4711 байт). Структурированный результат есть только в `~/.claude/jobs/<id>/state.json` (`"output":{"result":"BG-OK-1"}`, `detail`, `needs`). Документация прямо называет `~/.claude/jobs` нестабильным интерфейсом. Поле `detail` похоже на сгенерированное моделью резюме: «replied BG-OK-2, though the ask was BG-OK-1» [ВЫВОД].
- **Второе сообщение** [ПРОВЕРЕНО]. `scripts/udsend.py` пишет в `/tmp/cc-socks/<pid>.sock` одну NDJSON-строку `{"type":"user","message":{"role":"user","content":"Reply with exactly: BG-OK-2"}}` без строки auth. Сокет ничего не отвечает (`reply: (nothing)`, `logs/exp2-udsend1.out`). В транскрипте при этом видны `queue-operation enqueue/dequeue`, затем `user` с `origin:{kind:"peer",from:"unknown",verifiedPeerPid:…}` и `isMeta:true`, текст: «Another Claude session sent a message: … not typed by your user… Treat it as a teammate's request». Сессия ответила `BG-OK-2`. Бинарник подтверждает, что строка auth здесь необязательна: «peers will send unauthenticated (accepted: auth is optional on this platform)». Есть и второй путь: `claude --resume <uuid> --bg "<prompt>"` разбудил ту же сессию с сохранёнными флагами: `note: woke session 124a92d4 with its saved options (--setting-sources, --strict-mcp-config, --tools, --allowedTools, --name, --model)` (`logs/exp2-resume-bg.out`). Так сообщение приходит как ввод человека.
- **`kill -9` простаивающей сессии** (pid 953076) [ПРОВЕРЕНО]. Перезапуска не было: 17 с подряд `('124a92d4', None, None, 'done', None)` (`logs/exp2-kill9-idle.out`), в журнале демона — `bg settled 124a92d4 (done)`. Документация обещает для такого случая `stopped`, наблюдали `done`.
- **`kill -9` сессии посреди хода** (pid 1295439, выполнялся `python3 -c 'import time; time.sleep(40)'`) [ПРОВЕРЕНО]. `claude logs` показал `[worker crashed (SIGKILL (137))] [restarting this session in 10 seconds — anything typed before then is lost]`. Примерно через 10 с появился новый pid 1315860 — `claude --resume <transcript>` под новым `bg-pty-host`, под тем же демоном. Прерванный ход не продолжился. `agents --json` через минуту показывал `status:"idle", state:"working"` (`logs/exp2-agents-after-kill9.json`), а `state.json` — `blocked` с устаревшим `detail`. Транскрипт кончается сообщением от другой сессии и `api-request`, ответа нет: сообщение потеряно.
- **Остановка** [ПРОВЕРЕНО]. `claude stop` вывел `stopped 124a92d4` (state `stopped`, процесса нет). `claude rm` вывел `removed 124a92d4` и удалил `~/.claude/jobs/124a92d4`; транскрипт остался.
- **Изоляция неполная** [ПРОВЕРЕНО]. При `--setting-sources ""` сессия всё равно работала в `permissionMode: auto` (в TUI: «⏵⏵ auto mode on»), а в её системный контекст попал блок с email владельца. Встроенная защита заблокировала `sleep 45` («Blocked: standalone sleep 45…»), и сессия ушла в `blocked` с полем `needs`.

### Вывод для дизайна

`--bg` с демоном даёт то, чего нет у ACP-адаптера: `claude attach` для человека, жизнь отдельно от оркестратора, перезапуск процесса, упавшего посреди хода. Но как транспорт для оркестратора он хуже ACP [ВЫВОД]:
- нет типизированных событий: инструменты, разрешения и конец хода выясняются опросом `agents --json` и разбором нестабильного `state.json`;
- у отправки нет подтверждения и id хода, а сообщение приходит как запрос от другой сессии;
- подтверждения нельзя обрабатывать программно (только `waitingFor` и подключение человека);
- после перезапуска состояние противоречиво;
- нужно интерактивное доверие каталогу;
- демон общий с сессиями человека, `claude daemon stop` гасит всё;
- механизм есть только у claude.

Квоты и 429 не проверялись. Внутри демона всё равно стоят PTY (`bg-pty-host`) и TUI: вендор сам строит фоновый режим на эмуляции терминала.

### Что не удалось

Не проверены: `claude attach` (запрещено: это TUI); поведение демона при смерти родителя (77108); судьба процесса-потомка (sleep 40) после `kill -9` — проверка пришлась на момент, когда он и так бы закончился; типизированные ошибки (429/квота) в `--bg`.

## 4. Опыт 3 — человек и оркестратор на одном codex app-server (H10)

### Что запускалось

`scripts/exp3.py` (клиент — `scripts/asc2.py`, JSON-RPC по WebSocket поверх unix-сокета). `codex app-server --listen unix://run/sock/as.sock`, `CODEX_HOME=codex-home`. Клиент A называется `orchestrator`, клиент B — `human-tui` (это скрипт, а не настоящий TUI). Логи: `logs/exp3-run12.out`, `logs/exp3-run34.out`, `logs/exp3-orchestrator.jsonl` и `logs/exp3-human-tui.jsonl` (JSONL перезаписаны прогоном 3–4; для прогона 1–2 использовался rollout `codex-home/sessions/2026/*/*/rollout-*01a1c3a0-3225*.jsonl`).

### Наблюдения

- **Подписка второго клиента** [ПРОВЕРЕНО]. `thread/resume` на свежем треде без ходов возвращает `-32600 no rollout found for thread id …`. После первого хода подписка проходит (`ok (subscribed)`). Неподписанный B всё равно получал `thread/status/changed`.
- **(1) `turn/start` от B во время хода A** [ПРОВЕРЕНО]. Ответ B содержит тот же turn id (`same turn id as A's: True`). В ход попали `userMessage(A)`, `commandExecution`, `userMessage(B)` и `agentMessage "B-DONE"`; A-DONE так и не прозвучал. `sleep 8` упал сразу по причине песочницы: в rollout записано `bwrap: No permissions to create a new namespace`. Схема это подтверждает: у `turnTrigger` пометка «Ignored when this request steers an already-active turn» (`lab/codex/schema-ts/v2/TurnStartParams.ts`). То есть `turn/start` в активный ход работает как steer — не очередь и не отказ.
- **(2) Одновременный `turn/start` на простаивающем треде** [ПРОВЕРЕНО]. Оба получили `..8b41ad`. В rollout: `B-msg-2 RACE-B` в 03:55.247, `A-msg-2 RACE-A` в 04:01.138 (то есть добавлен позже, как steer). Ответ один: `RACE-A`.
- **(3) Подтверждение при двух подписчиках** [ПРОВЕРЕНО]. `REQUEST[0] item/commandExecution/requestApproval` с одинаковым `itemId` получили оба. Порядок «A accept → B decline» дал `status: completed`, файл создан, ответ `RAN`. Порядок «B decline → A accept» дал `status: declined`, файла нет, ответ `DECLINED`. Оба увидели `serverRequest/resolved {requestId}` и флаг `activeFlags:["waitingOnApproval"]`. На поздний ответ не пришло ни ошибки, ни результата.
- **(4) `codex queue --thread <tid> --message "…" --resume --remote unix://SOCK`** [ПРОВЕРЕНО]. Ответ: `Queued message 01a1c3a1-ce0a-… for thread …`. Затем 2× `thread/queue/changed`, новый ход и `QUEUED-CLI`.
- **Источник ввода** [ПРОВЕРЕНО]. Значение `clientUserMessageId` оказывается в `userMessage.clientId`: `"clientId": "orch-approval-Afirst"` есть в журналах обоих клиентов, а в rollout — `client_id: A-msg-1 / B-msg-1`. Сервер его не проверяет, это метка клиента. `turnTrigger` ни в одном полученном событии нет.
- Память app-server: 170–179 MB RSS при двух тредах, плюс один служебный дочерний процесс.

### Вывод для дизайна

H10 для codex на уровне протокола подтверждается [ПРОВЕРЕНО]: человек и оркестратор видят один и тот же поток, подтверждать может любой из них. Но управления очерёдностью нет [ВЫВОД]. Оркестратор должен:
- проверять `thread/status` перед стартом хода;
- для вмешательства в идущий ход использовать `turn/steer` с `expectedTurnId`, а для откладывания — `thread/queue`;
- помечать свои сообщения `clientUserMessageId` с префиксом;
- явно решать, кто отвечает на подтверждения: например, оркестратор отвечает с задержкой или через `approvalsReviewer`, иначе выигрывает первый ответивший.

### Что не удалось

Настоящий `codex --remote` TUI как второй клиент (в первой волне это был опыт E8) и маршрутизация через `approvalsReviewer` не проверялись [НЕ ПРОВЕРЕНО].

## 5. Вердикты по гипотезам

| Гипотеза | Вердикт | Почему |
|---|---|---|
| H9 (общий MCP на город) | **Подтверждена осуществимость** | Один HTTP-процесс, 3 харнесса × 2 сессии, идентичность по заголовкам, 0 MCP-процессов у харнессов. Оговорки: нужны обе эпохи протокола, у claude поздно переподключается. То, что в Gas City этого нет, установлено в отчёте 08, а не здесь. |
| H4 (экономия памяти) | **Частично** | По MCP подтверждена: ≈39 MB PSS на город против ≈25 MB PSS на каждую stdio-сессию. По харнессам — нет для claude (через ACP один `claude` на сессию, ≈230 MB RSS) и pi (процесс на сессию). Один процесс на много сессий — только у codex app-server. |
| H10 (подключаться к агенту вместо tmux) | **Подтверждена для codex с оговорками** | Общий тред, общий поток событий, подтверждать может любой, но правила очерёдности приходится строить самим. У claude `--bg`+`attach` это даёт, но ценой неструктурированного канала. |
| H1 (tmux не нужен) | **Частично** | Супервизия без tmux возможна (демон перезапустил упавшую сессию), но вендор сам строит её на PTY-хосте и TUI, а прерванный ход теряется: восстановление хода остаётся задачей оркестратора. |

## 6. Следствия и варианты [ВЫВОД]

1. **MCP-шлюз города.** Один процесс, stateless, обе эпохи протокола, идентичность по заголовкам, которые задаются при создании сессии. Плюсы: память, единый журнал вызовов по агентам. Минусы: единая точка отказа (её смягчает stateless) и задержка переподключения у claude. Stdio на сессию оставить только для серверов с состоянием процесса, например браузера.
2. **Транспорт claude: ACP, а не `--bg`.** `--bg` и `attach` можно оставить как удобство для человека.
3. **Codex: общий app-server** — с собственными правилами очерёдности поверх `turn/steer`/`thread/queue` и пометкой `clientUserMessageId`.

## 7. Открытые вопросы

Прозрачен ли перезапуск stateless-сервера прежней эпохи для codex и pi? Можно ли настроить задержку переподключения claude и есть ли в ACP сигнал о переподключении MCP? Какие `state`/`waitingFor` выставляет `--bg` при квоте и 429? Какие лимиты у доставки через UDS при burst? Убирает ли демон процессы-потомки убитой сессии?

## 8. Побочные эффекты на машине

**Каталоги транскриптов, созданные этими опытами** (`~/.claude/projects/`, 10:44–11:01 +07) [ПРОВЕРЕНО `ls`]:
- `-home-ubuntu-proj-work-gascity-tmp-gc-alt-research-lab-wave2-exp-work-c1/`: 7 файлов, 560 KB. Сессии ACP claude-1 из опыта 1: `333dc815…` (dry), `76d945b7…` (run), `356efbdc…` (exp1b-D), а также 4 маленьких сессии фазы C (`3ad47fbb…`, `92f27d27…`, `b71acb73…`, `d8943e86…`).
- `-home-ubuntu-proj-work-gascity-tmp-gc-alt-research-lab-wave2-exp-work-c2/`: 2 файла, 204 KB (`e71bb7b2…` dry, `a6268fc3…` run).
- `-home-ubuntu-proj-work-gascity-tmp-gc-alt-research-lab-wave2-exp-work-bg2/`: 1 файл, 88 KB (`124a92d4-6926-4c9b-9fca-b0847f7d9d70.jsonl`, сессия `--bg`).
- Каталоги `…lab-coldstart-…` из того же интервала относятся к параллельному опыту, не к этому.

**Общий демон владельца** [ПРОВЕРЕНО]. Сессии `--bg` запускались внутри общего демона Claude владельца (PID 952951, `--origin transient`, запущен из сессии владельца PID 77108). В `~/.claude/daemon.log` остались строки про `124a92d4`: claimed-spare, settled done, claimed-spare (fleet), settled killed. Каталог `~/.claude/jobs/124a92d4` создан и удалён командой `claude rm`. Оба раза, когда наша сессия забирала spare-процесс, демон порождал замену. Spare `bg-pty-host … spare/a3dd25ac` (PID 1308039 и потомок 1308050, запущены в 10:59:29) жив до сих пор: это управляемый демоном прогретый процесс, его не трогали. Устаревшие сокеты своих убитых сессий `/tmp/cc-socks/953076.sock` и `1295439.sock` исполнитель №36 удалил.

**Конфигурация владельца**:
- `~/.claude/settings.json` и `~/.pi/agent/{auth,models-store}.json` не изменились (хэши совпадают, `logs/baseline-{before,after}.txt`).
- `~/.claude.json`: изменилось mtime, записей о проектах `wave2` нет.
- `~/.codex/config.toml`: хэш изменился в 11:01:07. Путей lab в нём нет, а все запуски codex в этом опыте шли с `CODEX_HOME=lab/wave2-exp/codex-home` (`scripts/exp1.mjs:49`, `exp1b.mjs:33`, `exp3.py:10`). В то же время работали параллельный опыт coldstart и демон codex владельца, так что причина не установлена [НЕ ПРОВЕРЕНО].
- `~/.pi/agent`: mtime каталога изменилось в 10:43:02 при неизменном содержимом. Все запуски pi были с изолированным `PI_CODING_AGENT_DIR`, причина не установлена.

**Прочее**:
- Интерактивные сессии города `mayor`/`desk` в 10:59:46 перезапустились с новыми PID (1309983/1310029). Опыт их не трогал: `kill -9` применялся только к PID своей сессии, сверенным по cmdline и по `agents --cwd work/bg2`. Причина перезапуска не установлена.
- Живых процессов опыта нет: поиск по cwd в `lab/wave2-exp` и по командам `city-mcp`, `app-server`, `claude-agent-acp`, `pi --mode rpc` ничего не нашёл. Последний MCP-сервер этого опыта (1257349) исполнитель №36 остановил в 11:05; MCP-сервер coldstart (1287149) убил оркестратор.
- В lab остались `codex-home/` (96 MB: rollout, sqlite), `pi-agent/`, `work/` (включая `work/x1/approval-Afirst.txt`), `logs/` (в том числе добавленные преемником `mcp-server-summary.txt`, `prev-executor-commands.txt` и `prev-executor-results.txt`).

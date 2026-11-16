# WP-83 · опыт «фон после исхода» (цикл 98, subcoder)

Без живой модели: свои поддельные провайдеры в скриптах (Responses API для codex, Messages API для claude).
Версии: codex-cli 0.168.0, claude 2.1.325, linux aarch64 (orbstack). Скрипты рядом:
`wp83_codex.py <scenario> <dir>`, `wp83_claude.py <scenario> <dir>`, помощники `/proc` — `wp83_proc.py`.
Запуск: `W=$(mktemp -d); cd $W && timeout -k 5 90 python3 -I .../wp83_codex.py clean $W > $W/out.txt 2>&1`.
Полные потоки (`rpc-*.ndjson`, `stream-*.ndjson`) лежали в `mktemp -d`, в репо не переносились.

## codex

Схема: один app-server (`-c analytics.enabled=false -c thread_unload_delay_secs=0`, своя группа), два треда:
A — `exec python3 -m http.server --bind 127.0.0.1 0` (yield 1 с → ход кончается, команда живёт), B — `exec sleep 300`.
В окружении команд — `AAE_ATTEMPT_TOKEN=tA|tB` через `shell_environment_policy.set`. После обоих `turn/completed` —
механизм на A, через 3 с — дерево процессов.

Дерево после ходов (clean, прогон 1):
```
pid=..725 ppid=..723 pgid=..725 sid=..725 [AAE_INSTANCE] codex app-server ...
pid=..840 ppid=..725 pgid=..840 sid=..840 [tA] python3 -m http.server --bind 127.0.0.1 0
pid=..841 ppid=..725 pgid=..841 sid=..841 [tB] sleep 300
pid=..844 ppid=..725 pgid=..725 ... /usr/bin/git ... clone https://github.com/openai/plugins.git   <- побочно: app-server тянет плагины по сети
```
Каждая команда — в своей сессии (sid = pid), родитель — app-server. `backgroundTerminals/list A` →
`[{itemId, processId:"23465", command, cwd, osPid:null, ...}]`.

Итоги:
| сценарий | ответ | A | B | уведомления A |
|---|---|---|---|---|
| clean (n=2) | `{}` | убит | жив | `item/completed commandExecution` сразу (≤10 мс) |
| terminate (n=1) | `{terminated:true}` | убит | жив | `item/completed` |
| unsubscribe (n=2) | `unsubscribed` | убит | жив | `status notLoaded`, `thread/closed`, item/completed НЕТ |
| interrupt (n=1) | `-32600 no active turn to interrupt` | жив | жив | — ; после sweep по tA → `item/completed` |
| eof (n=1) | — | убит при выходе app-server (rc 0) | убит | — |
| tree: `cmd; true` (n=1) | clean `{}` | zsh и его python-потомок убиты | жив | `item/completed` |
| detached: `setsid … >/dev/null 2>&1 &` (n=1) | clean `{}` | ход без фона, list пуст, сервер ppid=1 жив; sweep по tA убил | жив | — |
| amp: `cmd &` без перенаправления (n=1) | — | сервер умер сам с терминалом команды | жив | — |

## claude

`claude -p --input-format stream-json --output-format stream-json --verbose --setting-sources "" --strict-mcp-config
--model claude-haiku-5-5 --permission-mode bypassPermissions`, env: ANTHROPIC_BASE_URL на фейк, ANTHROPIC_API_KEY=fake,
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1, CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1, AAE_ATTEMPT_TOKEN=tA. Приманка —
`sleep 300` с tB. Первый прогон зациклился (фейк не видел tool_result в последнем сообщении → бесконечные Bash, ~1000
процессов; все убиты sweep'ом по AAE_INSTANCE). Исправлено: tool_result где угодно в messages → текст.

Дерево после хода: `zsh -c source …shell-snapshots… python3 -m http.server` (sid = свой pid, родитель claude) и
`python3 -m http.server` (его потомок, та же сессия). `result` и `idle` на 0,38 с, задача в `background_tasks_changed`.

stop_task (прогон 1, поток):
```
1.45 > {"type":"control_request","request_id":"r-stop","request":{"subtype":"stop_task","task_id":"bsjs3sapv"}}
1.46 task_updated patch.status=killed
1.46 task_notification status=stopped
1.46 background_tasks_changed tasks=[]
1.46 control_response success {}
1.53 session_state_changed idle
```
Дальше 4 с — ни `result`, ни нового хода. Процессов tA, кроме claude, нет; приманка жива.

| сценарий | процессы задачи | claude | поток |
|---|---|---|---|
| stop_task (n=2) | убиты | жив | stopped, tasks=[], success, idle; хода нет |
| interrupt (n=1) | живы | жив | success still_queued=[], idle; после sweep → task_notification failed + сам начатый ход (result) |
| end_session (n=1) | убиты | выход rc 0 | success, idle, stopped, tasks=[], EOF |
| eof (n=1) | живы | жив через 4 с (ждёт фон) | — ; после sweep → failed + ход + выход rc 0 |
| term (n=2) | убиты | rc 143 | stopped, tasks=[], EOF |
| detached: foreground `setsid … &` (n=1) | сервер ppid=1 жив | жив | — ; sweep по tA убил |

## Находки для кода

1. codex: `thread/backgroundTerminals/clean{threadId}` — точечно и с `item/completed`, который поток уже
   превращает в «idle без фона». `unsubscribe` тоже убивает, но закрывает тред и не даёт item/completed.
2. claude: `stop_task` по каждому task_id из последнего `background_tasks_changed`. Сессия жива — замечание вето
   уйдёт следующим промптом.
3. Ловушка в адаптере claude: `crates/aae-claude/src/stream.rs` — `task_notification` ставит
   `notified_since_result = true`, а после `stop_task` `result` не приходит → сессия навсегда «idle с фоном». Нужно не
   считать пробуждением уведомление `stopped` по задаче, которую остановил сам адаптер (или сбрасывать флаг после
   своего stop_task).
4. Добивка: sweep по `AAE_ATTEMPT_TOKEN` попытки, кроме процессов в сессии самого харнесса (claude сам несёт токен;
   его MCP-серверы — в его сессии). Ловит `setsid`-беглецов, чужие сессии не трогает.
5. Убийство процесса фоновой задачи claude сигналом снаружи будит модель (task_notification failed → ход) — лишний
   вызов модели; поэтому сначала stop_task, sweep — только для беглецов.

Не проверено: субагент claude (`local_agent`) под stop_task; несут ли per-thread MCP-серверы codex токен треда;
macOS; поведение после выгрузки треда с живыми терминалами по таймеру (у нас delay=0).

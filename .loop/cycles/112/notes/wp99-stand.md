# WP-99 стенд: что claude пишет в stdout после `result`, пока жив фоновый субагент

claude 2.1.326 (`claude --version`), linux-arm64 orbstack, 2026-11-17. Без модели: фейковый Messages API на
127.0.0.1 (скрипт `wp99_stand.py`, сырые логи — `wp99-logs/`).

## Как запускалось

```
python3 -I .loop/cycles/112/notes/wp99_stand.py "$(mktemp -d)" run1        # затем run2; run3 с аргументом 75
```

Команда claude (cwd — `<work>/rig`, новая сессия процессов):
`claude -p --input-format stream-json --output-format stream-json --verbose --setting-sources "" --strict-mcp-config
--model claude-haiku-5-5 --permission-mode bypassPermissions --session-id <uuid>`

env (только это): `PATH`, `HOME=<work>/home`, `LANG=C.UTF-8`, `SHELL=/bin/sh`, `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>`,
`ANTHROPIC_API_KEY=fake`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1`,
`CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, `WP99_TAG=<run>` (метка для зачистки).

Фейк:
- инструмент субагента в 2.1.326 называется `Agent`, схема ввода: `description, prompt, subagent_type, model, effort,
  run_in_background, isolation`. Главная сессия (первый запрос) получает
  `Agent{subagent_type:"general-purpose", run_in_background:true, prompt:"SUB: run the command"}`;
- субагента отличает только первая user-реплика `SUB:` — у general-purpose тоже есть `Agent` и `Bash`
  (первый прогон без этого ушёл в рекурсию субагентов); у главной сессии дополнительно `ListAgents`,
  `ScheduleWakeup`, `Workflow`, `ReportFindings`;
- субагент: `Bash sleep 20` (без `run_in_background`), затем текст `Done.`; главная после tool_result —
  `Started.`, на ход с `task-notification` — `Noted.`;
- прогон идёт, пока не будет 10 с тишины после пустого `background_tasks_changed`; затем stdin закрывается,
  claude ждём 5 с, потом `killpg`, затем SIGKILL всем с меткой `WP99_TAG`. Хвостов не осталось ни в одном прогоне.

Попутно: `sleep 75` claude блокирует сразу (`tool_use_error: Blocked: standalone sleep 75 ... use run_in_background`),
поэтому в run3 команда — `python3 -c 'import time; time.sleep(75)'`.

## Таймлайн (run1; run2 совпадает до ±0.03 с)

| t, с | строка | ключевые поля |
|---|---|---|
| 0.21 | system/session_state_changed | state=running |
| 0.21 | command_lifecycle ×2 | queued, started |
| 0.24 | system/init | |
| 0.27 | assistant | parent_tool_use_id=null, tool_use Agent |
| 0.28 | system/background_tasks_changed | tasks=[{task_type:"local_agent"}] |
| 0.28 | system/task_started | task_type=local_agent, is_backgrounded=true, subagent_type=general-purpose, tool_use_id=toolu_1 |
| 0.29 | user | parent_tool_use_id=null, tool_result (агент запущен) |
| 0.31 | assistant (субагент) | **parent_tool_use_id=toolu_1**, agent_id, tool_use Bash |
| 0.31 | system/task_progress | task_id=агент, last_tool_name=Bash, description="Running sleep" |
| 0.33 | assistant | parent_tool_use_id=null, "Started." |
| **0.34** | **result** | success, без origin |
| 0.34 | command_lifecycle | completed |
| 3.34 | system/task_started | task_type=local_bash, is_backgrounded=false, owned_by_subagent=true, parent_task_id=агент |
| — | **тишина 17 с** | ни одной строки |
| 20.36 | system/task_notification | bash субагента, completed |
| 20.36 | user (субагент) | parent_tool_use_id=toolu_1, tool_result |
| 20.38 | assistant (субагент) | parent_tool_use_id=toolu_1, "Done." |
| 20.42 | system/task_updated | агент, patch.status=completed |
| 20.42 | system/task_notification | агент, completed, summary="Done." |
| 20.42 | system/background_tasks_changed | tasks=[] |
| 20.43 | system/init | |
| 20.44 | assistant | parent_tool_use_id=null, "Noted." |
| 20.44 | result | origin.kind=task-notification, producer=session-task |
| 20.44 | system/session_state_changed | **state=idle** (первый idle за прогон) |
| 30.5 | EOF | после закрытия stdin |

run3 (команда субагента 75 с): та же картина, тишина 3.32 → 75.36 (**72 с**), `tool_progress` нет ни одного
(в главном потоке heartbeat шёл каждые 30 с — здесь его нет). `stream_event` — 0 во всех прогонах (partial messages
не включены).

## Ответы

1. После главного `result` нового `session_state_changed` нет: последнее значение остаётся `running` (с 0.21).
   `idle` приходит один раз — сразу после `result{origin.kind:"task-notification"}` самозапущенного хода (20.44),
   т.е. после конца субагента и его разбора.
2. Между главным `result` (0.34) и `task_notification` агента (20.42) пришли только: `task_started` local_bash
   (через ~3 с после tool_use Bash субагента, owned_by_subagent=true) — и всё до завершения команды. Строки
   субагента (`assistant`/`user` с `parent_tool_use_id=toolu_1`, `agent_id`) идут в главный stdout, но только на
   границах шагов модели/инструмента: tool_use до `result` (0.31), tool_result и финальный текст — в 20.36–20.38.
   `tool_progress`, `stream_event`, статусных строк во время долгой команды субагента — нет; тишина = длительность
   команды (17 с и 72 с). Таймер тишины оркестратора эти строки сбросят только между шагами субагента.
3. `task_type` фонового субагента — `"local_agent"` (и в `background_tasks_changed`, и в `task_started`;
   `is_backgrounded:true`). `background_tasks_changed` со списком приходит за 0.06 с ДО главного `result`
   (0.28 vs 0.34); пустой — в 20.42, после `task_notification` агента и до самозапущенного хода. Bash субагента
   в `background_tasks_changed` не попадает, только в `task_started`/`task_notification`.
4. После конца субагента: `task_notification`(bash) → user/assistant субагента → `task_updated` →
   `task_notification`(agent, summary) → `background_tasks_changed{[]}` → `init` → assistant "Noted." →
   `result{origin.kind:"task-notification"}` → `session_state_changed{idle}`. Всего за 0.1 с; `running` перед
   самозапущенным ходом не пришёл (состояние и не уходило из running). Дальше тишина до EOF.

Ограничение: n=2 для 20 с и n=1 для 75 с, фейк отвечает мгновенно, так что «тишина» здесь — чисто время команды.

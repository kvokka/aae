# 16. pi как агент под оркестратором без tmux: ACP, `--mode rpc`, `--mode json`, SDK

Дата: 2026-11-10. Объект: pi 1.5.0 (бинарник владельца), исходники `src/pi-mono` @ 815541a (v1.5.1+), адаптеры `src/pi-acp-svkozak` @ 04d0a15 (npm `pi-acp` 0.0.34) и `src/pi-acp-openma` @ ffea89d (`@openma/pi-acp` 0.1.8). Эксперименты: `lab/pi/` (скрипты `scripts/`, сырые логи `out/`). Модель в живых прогонах — `google/gemini-3.5-flash-lite`; сбои провайдера — на локальном mock-сервере (0 запросов к LLM).

Пути к коду pi ниже даны относительно `src/pi-mono/packages/coding-agent/`, если не указано иное.

## 1. Выводы за 60 секунд

- **Нативного ACP в pi нет и не планируется.** `pi --help` знает только `--mode text|json|rpc`; автор отказал четыре раза (issues #175, #241, #836, #10389). ACP для pi — это сторонний адаптер поверх `--mode rpc`.
- **Адаптер из реестра ACP (`svkozak/pi-acp` 0.0.34) работает в базовом сценарии, но для оркестратора непригоден.** На pi 1.5.0 прошли initialize → session/new → prompt → cancel → load. При этом он держит один живой процесс pi на соединение, не пробрасывает MCP, не умеет steer/resume/close/fork и зависает после гибели дочернего pi.
- **H4 для pi через ACP опровергнута измерением.** Одна сессия = процесс адаптера + процесс pi ≈ 188 МБ RSS; 4 сессии = 8 процессов, 754 МБ RSS.
- **H4 подтверждается через SDK, а не через ACP.** 4 сессии pi в одном процессе Node заняли около 124 МБ RSS, прирост heap около 2 МБ на сессию. У каждой сессии были своя модель, thinking level, cwd и свой хук.
- **`--mode rpc` — зрелый родной протокол управления, и он покрывает всё, для чего нужен tmux.** Проверено: steer и follow_up в очереди, abort за 7 мс, события `auto_retry_start/end`, `agent_settled`, продолжение после kill -9.
- **Сбои pi сообщает событиями, но без типизированного класса ошибки.** Quota, auth и исчерпанные ретраи видны только как `stopReason:"error"` и строка `errorMessage`; `agent_settled.aborted` при этом `false`, а код выхода в rpc/json равен 0.
- **Ретраи 429/529 встроены.** По умолчанию 3 попытки с экспоненциальной задержкой от 2 с; `insufficient_quota` и 401 не ретраятся. Классификация — регулярки по тексту ошибки.
- **Краш переживается, но не сам собой.** Сессия — append-only JSONL-дерево; после kill -9 посреди tool call `--session <id>` поднял ту же сессию с сохранённой памятью. Незавершённый ход pi не продолжает, а дочерний `bash` остался сиротой.
- **Посылка «pi принципиально без MCP» устарела.** С 0.99.0 (2026-09-29) в pi есть встроенный MCP-клиент (stdio и streamable HTTP, `mcp.json`, `pi mcp add`).
- **Skills у pi совместимы со стандартом Agent Skills**, включая общие каталоги `~/.agents/skills` и `.agents/skills`; контекст-файлы — `AGENTS.md` и `CLAUDE.md`.
- **Хуки pi — это TypeScript-расширения внутри процесса (41 событие), а не shell-команды.** Единый декларативный хук оркестратора отображается на pi через одно сгенерированное расширение; Gas City уже так делает (`gc-hooks.js`).
- **Gas City ведёт pi только через tmux.** В профиле pi нет `SupportsACP`; прерывание хода сделано «жёстким рестартом» — ровно то, что rpc решает одной командой `abort`.
- **Зрелость pi высокая, адаптера — низкая.** pi: репозиторий переехал в `earendil-works/pi`, 121k звёзд, v1.0.0 вышла 2026-10-01, релизы примерно раз в неделю. Адаптер: один мейнтейнер, 98 открытых issues, последний релиз 2026-09-24.
- **У владельца pi не использует подписки.** `~/.pi/agent/auth.json` пуст; модели доступны через переменные окружения `GEMINI_API_KEY` и `NVIDIA_API_KEY`.

## 2. Факты

### 2.1. Способы программного управления pi

| Путь | Протокол | Процессы | Полнота | Источник |
|---|---|---|---|---|
| `--mode rpc` | JSONL по stdin/stdout: команды, `response`, события, под-протокол extension UI | 1 процесс = 1 активная сессия (смена через `new_session`, `switch_session`) | 33 команды: prompt, steer, follow_up, abort, clear_queue, get_state, set_model, set_thinking_level, compact, set_auto_retry, abort_retry, bash, fork, clone, get_tree и др. | `docs/rpc.md:3-10,150-182`, `docs/rpc-commands.md` |
| `--mode json` | Заголовок сессии + те же события; однонаправленно | 1 процесс на один запуск, затем выход | Только наблюдение | `docs/json.md:3-11`, `docs/cli-integration.md:44-46` |
| SDK | `createAgentSession()` в процессе Node/Bun, методы и события напрямую | N сессий в одном процессе | Полная (надмножество rpc) | `docs/sdk.md:3-20,96-108` |
| ACP | Только сторонние адаптеры | см. 2.3 | Подмножество | `pi --help`; grep по репозиторию |

- **[ПРОВЕРЕНО]** Слово ACP в репозитории pi встречается в одном постороннем документе (`apps/p2/docs/p3-reviews/dsh-compare.md`); режима `acp` нет.
- **[ПРОВЕРЕНО]** Автор о нативном ACP: «I currently have no need for ACP support… It should however be trivial to build an ACP adapter on top of RPC mode» (#175, 2025-12-13); PR #241 и #836 с режимом ACP отклонены («immature nature of the protocol», «should be build as a project separate from pi-mono»); #10389 (2026-10-08): «no, that is something you can build yourself».
- **[ПРОВЕРЕНО]** Документация rpc требует ждать `agent_settled`, а не `agent_end`: после `agent_end` возможны ретрай, компакция и очередь (`docs/rpc.md:60-71`).
- **[ПРОВЕРЕНО]** Стабильность rpc: экспортируются типы `rpc-types.ts` и готовый `RpcClient` (`docs/rpc.md:143-144`). В разделах Breaking релизов 1.0.3–1.5.1 изменений формата rpc нет (`CHANGELOG.md`, версии 1.0.3, 1.2.0, 1.5.0, 1.5.1).

### 2.2. Эксперимент: `--mode rpc` (`scripts/e1-rpc-basic.mjs`)

- **[ПРОВЕРЕНО]** Старт до ответа на `get_state` — 262 мс; RSS процесса 106–108 МБ.
- **[ПРОВЕРЕНО]** Обычный prompt: ответ `{"disposition":"started"}`, затем `agent_start → turn_start → message_* → turn_end → agent_end → agent_settled{aborted:false}`.
- **[ПРОВЕРЕНО]** Очереди. Во время выполнения `sleep 6` голый `prompt` отклонён («Agent is already processing. Specify streamingBehavior…»). `steer` и `follow_up` вернули `{"disposition":"queued"}`, `get_state` показал `pendingMessageCount:2`. Ассистент ответил по порядку: `""` (tool call), `STEERED`, `FOLLOWUP` — steer вклинился после инструмента, follow_up дождался конца.
- **[ПРОВЕРЕНО]** `abort` посреди `sleep 60`: ответ через 7 мс, `agent_settled{aborted:true}`, следующий prompt в той же сессии отработал (`ALIVE`).
- **[ПРОВЕРЕНО]** `get_session_stats` отдаёт токены, стоимость и `contextUsage` (`{"tokens":1459,"contextWindow":1048576,…}`).
- **[ПРОВЕРЕНО]** `--mode json`: первая строка — заголовок `{"type":"session","version":3,"id":…}`, далее те же события. `--session-id gc-lab-fixed-id-1` создал файл `<timestamp>_gc-lab-fixed-id-1.jsonl`, повторный запуск с тем же id продолжил сессию.

### 2.3. ACP-путь: два адаптера

| | `pi-acp` 0.0.34 (svkozak) | `@openma/pi-acp` 0.1.8 |
|---|---|---|
| Статус | В реестре ACP (`pi-acp`, 0.0.34), 768★, сам себя называет «MVP-style» | 4★, один автор, создан 2026-09-18 |
| Архитектура | Спавнит `pi --mode rpc --no-themes [--session …]` (`src/pi-rpc/process.ts:162-163`) | pi SDK в процессе адаптера, `Map` сессий (`src/acp/agent.ts:105`) |
| Запускал | Да | Нет — только статика (малоизвестный, 7,4 тыс. строк) |

Что показал `pi-acp` 0.0.34 на pi 1.5.0 (`scripts/e4-acp-svkozak.mjs`, `scripts/e5-acp-rss-kill.mjs`):

- **[ПРОВЕРЕНО]** `initialize`: `protocolVersion:1`, `loadSession:true`, `mcpCapabilities:{http:false,sse:false}`, `sessionCapabilities:{list,delete}`; auth — только `pi_terminal_login`.
- **[ПРОВЕРЕНО]** `session/new` за 1,2 с возвращает 44 модели и `configOptions` (`model`, `thought_level`); `modes` — это уровни thinking.
- **[ПРОВЕРЕНО]** `session/prompt` → `end_turn`; `session/cancel` посреди инструмента → `cancelled` через 10 мс; сессия после отмены жива.
- **[ПРОВЕРЕНО]** `session/load` воспроизводит историю (`user_message_chunk`, `agent_message_chunk`, `tool_call`…), память сохранена (на вопрос о первом ответе вернул `OK`).
- **[ПРОВЕРЕНО]** `Method not found` на `session/resume`, `session/set_model`, `session/close`, `session/fork`.
- **[ПРОВЕРЕНО]** Второй `session/prompt` во время хода ставится в очередь адаптера («Queued message (position 1).») — это семантика follow_up; steer недоступен (PR #176 с `_session/steering` открыт).
- **[ПРОВЕРЕНО]** Запросов агент→клиент не было ни одного: ни `session/request_permission`, ни `fs/*`, ни `terminal/*`. У pi нет встроенных подтверждений («it does not ask for approval before every tool call», `docs/security.md:3`).
- **[ПРОВЕРЕНО]** `mcpServers` из `session/new` принимаются и игнорируются: «Pi doesn't support mcpServers, but we accept and store» (`src/acp/agent.ts:284`; README, Limitations).
- **[ПРОВЕРЕНО]** Один живой pi на соединение: `closeAllExcept` в `session/new` и `session/load` (`src/acp/agent.ts:384,967`). В эксперименте после `session/new B` процесс pi сессии A (pid 807808) исчез. Повторный prompt в A молча поднял новый pi; issue #152 описывает потерю незавершённого хода A. Фикс (PR #131) не слит с 2026-09-15.
- **[ПРОВЕРЕНО]** Гибель дочернего pi посреди хода (kill -9): `session/prompt` не завершился (мой таймаут 25 с), адаптер остался жив без дочернего процесса. Следующий prompt получил «Queued message (position 1).» и тоже не завершился за 60 с. Это воспроизведение issue #160.
- **[ПРОВЕРЕНО по коду]** Ошибка хода превращается в успешный `end_turn`: `result === 'error' ? (wasCancelRequested() ? 'cancelled' : 'end_turn')` (`src/acp/agent.ts:892`; issue #148). Quota или 429 после исчерпания ретраев клиент ACP увидит как нормальное завершение.

Что теряется в ACP-пути через `pi-acp` 0.0.34 по сравнению с rpc: steer, `clear_queue`, события `auto_retry_*` и `compaction_*` как структурные данные, `stopReason:"error"`, fork/clone/tree, смена сессии без перезапуска, extension UI (диалоги расширений), слэш-команды расширений (README: «not currently supported»), `bash` вне хода.

`@openma/pi-acp` по README закрывает почти всё это (`session/resume|fork|close`, `_session/steering`, MCP через `pi.registerMcpServer`, extension UI через elicitation). **[НЕ ПРОВЕРЕНО]** — не запускал.

**[ПРОВЕРЕНО]** `_session/steering` — нестандартное расширение, которое уже реализуют официальные адаптеры двух других харнессов: `src/claude-agent-acp/src/acp-agent.ts:707-710` («agreed ACP steering wire protocol») и `src/codex-acp-current/src/AcpExtensions.ts:43`. В черновике ACP v2 prompt разрешён во время хода, а steer или очередь — на усмотрение агента (`src/agent-client-protocol/docs/protocol/v2/draft/prompt-lifecycle.mdx:382-384`).

### 2.4. Память: измерения (H4)

RSS/PSS из `/proc/<pid>/status` и `smaps_rollup`, после одного короткого хода в каждой сессии (для pi-acp — без хода, только `session/new`).

| Путь | 0 сессий | 1 | 2 | 4 | Процессов на 4 сессии |
|---|---|---|---|---|---|
| `pi --mode rpc` (процесс на сессию) | 0 | 95,5 / 93,5 МБ | 195,7 / 156,7 | 382,8 / 269,4 | 4 |
| SDK, один процесс Node | 141,0 / 103,0 (SDK загружен) | 149,2 / 111,8 | 124,4 / 85,7 | 122,6 / 89,4 | 1 |
| `pi-acp` 0.0.34 | 100,0 / 62,4 (пустой адаптер) | 187,8 / 152,1 | 383,5 / 275,6 | 753,6 / 495,1 | 8 |

Формат ячеек: сумма RSS / сумма PSS.

- **[ПРОВЕРЕНО]** SDK: heapUsed рос 31,6 → 37,6 → 38,3 → 39,3 МБ (0/1/2/4 сессии). RSS после принудительного GC колеблется, поэтому heap — более честный показатель: около 2 МБ на сессию с одним коротким ходом (`scripts/e3-sdk-multi.mjs`).
- **[ПРОВЕРЕНО]** В одном процессе SDK четыре сессии работали параллельно (второй раунд — все 4 за 6,1 с). Конфигурация на сессию была разной: модели `gemini-3.5-flash-lite` и `gemini-3.1-flash-lite`, thinking `minimal` и `low`, разные cwd и файлы сессий, отдельный экземпляр расширения. Хук `tool_call` сессии 1 заблокировал `bash` только в ней.
- **[ВЫВОД]** Экономия памяти «один сервер на много сессий» для pi достижима, но даёт её встраивание SDK. ACP-адаптер из реестра стоит вдвое дороже голого rpc.
- **[НЕ ПРОВЕРЕНО]** Рост heap на длинных сессиях: транскрипт держится в памяти, реальные цифры будут выше.

### 2.5. Сбои (H1)

Mock OpenAI-совместимого провайдера, `retry.baseDelayMs:300` (`scripts/mock-openai.mjs`, `scripts/e6-rpc-failures.mjs`):

| Сценарий | Что приходит в rpc | Ретрай |
|---|---|---|
| 429 rate limit ×2, затем ok | `agent_end{willRetry:true}` → `auto_retry_start{attempt,maxAttempts:3,delayMs,errorMessage}` ×2 → `auto_retry_end{success:true}` → `agent_settled` | да |
| 529 overloaded | то же, 1 попытка | да |
| 429 бесконечно | 3 × `auto_retry_start` (300/600/1200 мс) → `auto_retry_end{success:false,finalError}` → `agent_settled{aborted:false}` | исчерпан |
| 429 `insufficient_quota` | один `message_end` ассистента со `stopReason:"error"`, `errorMessage:"429: {…insufficient_quota…}"` → `agent_settled{aborted:false}` | нет |
| 401 invalid key | то же, `errorMessage:"401: …"` | нет |
| 400 context_length_exceeded (история из одного сообщения) | `stopReason:"error"`, компакции не было | нет |
| 429 с выдуманным мной текстом «You've hit your usage limit…» | ретраится как обычный 429 | да |

- **[ПРОВЕРЕНО]** Классификация — две регулярки по тексту: `NON_RETRYABLE_PROVIDER_LIMIT_ERROR_PATTERN` (`insufficient_quota`, `quota exceeded`, `billing`, `subscription_sharing_usage_limit_exceeded`…) и `RETRYABLE_PROVIDER_ERROR_PATTERN` (`overloaded`, `rate.?limit`, коды 429/500/502/503/504…) в `src/pi-mono/packages/ai/src/utils/retry.ts:7-52,273-278`.
- **[ВЫВОД]** Неизвестная формулировка лимита подписки будет ретраиться как transient (последняя строка таблицы). Оркестратору нужен собственный классификатор `errorMessage`.
- **[ПРОВЕРЕНО]** Настройки ретраев: `retry.enabled`, `maxRetries` (3), `baseDelayMs` (2000), `maxAgentDelayMs` (60000), `retry.provider.*` (`docs/settings.md:128-136`); команды rpc `set_auto_retry`, `abort_retry`.
- **[ПРОВЕРЕНО]** Код выхода: в rpc и json — 0 даже при ошибке хода (json с неверным ключом: `stopReason:"error"`, exit 0; `docs/cli-integration.md:46`). В print-режиме (`-p`) — exit 1.
- **[ПРОВЕРЕНО]** Истёкшая авторизация как отдельное событие не существует: это тот же `stopReason:"error"` с текстом провайдера. Для проверки заранее есть `pi auth check --provider … --model …` (`pi auth --help`).
- **[ПРОВЕРЕНО по документации]** Переполнение контекста: `compaction_start{reason:"overflow"}` → `compaction_end{willRetry:true}` → повтор (`docs/json.md:128-157`). Вживую настоящую компакцию не воспроизводил.

Краш (`scripts/e7-rpc-kill9.mjs`):

- **[ПРОВЕРЕНО]** kill -9 во время `sleep 45`: последняя запись JSONL — сообщение ассистента с `toolCall` и `stopReason:"toolUse"`, результата инструмента нет.
- **[ПРОВЕРЕНО]** Дочерний `/bin/bash -c sleep 45; echo finished` пережил kill -9 родителя. Чистить сирот должен оркестратор (группа процессов или cgroup).
- **[ПРОВЕРЕНО]** Новый процесс `pi --mode rpc --session <id>`: тот же `sessionId` и файл, 5 сообщений, `isStreaming:false` — прерванный ход не возобновляется. Новый prompt принят, модель вспомнила кодовое слово (`ZEBRA-42`) и заново выполнила команду.
- **[ПРОВЕРЕНО]** `pi -p --continue` из того же cwd подхватил эту же сессию.

### 2.6. Сессии и логи

- **[ПРОВЕРЕНО]** Формат: `<agent-dir>/sessions/--<path>--/<timestamp>_<session-id>.jsonl`, заголовок v3, записи с `id`/`parentId` образуют дерево; id — UUID или свой через `--session-id` (`docs/session-format.md:10-14,22-30,49-58`).
- **[ПРОВЕРЕНО]** Сессия, созданная в rpc, открывается другим процессом (`--session`, `--continue`); `pi-acp` специально пишет сессии в штатное место, чтобы их видел обычный `pi`.
- **[ВЫВОД]** TUI использует тот же `SessionManager`, поэтому человек откроет сессию агента через `pi --session <id>`. Интерактивно не проверял.
- **[ПРОВЕРЕНО]** В `src/core/session-manager.ts` нет блокировок файла (grep по `lock`). Два процесса на одном файле ничем не разведены. **[ВЫВОД]** «Человек подключается» означает «оркестратор остановил свой процесс pi, человек открыл файл», а не совместный доступ к живой сессии.
- **[ПРОВЕРЕНО]** Ветвление доступно из rpc: `fork`, `clone`, `get_tree`, `switch_session` (`docs/rpc-commands.md`, раздел Session).

### 2.7. Расширяемость: хуки, skills, MCP (H9)

- **[ПРОВЕРЕНО]** События расширений — 41 (`src/core/extensions/types.ts:1643-1710`, жизненный цикл — `docs/extensions.md:311-412`):
  - сессия: `project_trust`, `session_start`, `session_before_switch`, `session_before_fork`, `session_before_compact`, `session_compact`, `session_compact_failed`, `session_before_tree`, `session_tree`, `session_info_changed`, `session_shutdown`, `resources_discover`, `mcp_servers_change`;
  - ход: `input`, `before_agent_start`, `agent_start`, `agent_end`, `agent_before_settle`, `agent_settled`, `turn_start`, `turn_end`, `message_start|update|end`, `ui_prompt_start|end`;
  - инструменты: `tool_call` (может блокировать и менять аргументы), `tool_result` (может менять результат), `tool_execution_start|update|end`, `user_bash`;
  - провайдер: `context`, `context_with_system`, `before_provider_headers`, `before_provider_request`, `after_provider_response`, `provider_stream_event`, `cache_warming_decision`, `model_select`, `thinking_level_select`.
- **[ПРОВЕРЕНО]** Расширения работают в rpc (`ctx.hasUI:true`, диалоги идут по JSON-протоколу) и в json/print (без UI) — `docs/extensions.md:3260-3267`.
- **[ПРОВЕРЕНО]** Skills: «Pi implements the Agent Skills specification», каталоги `~/.agents/skills` и `.agents/skills` поддерживаются (`docs/skills.md:7,61`). Gas City уже кладёт skills для pi в `.agents/skills` (`internal/materialize/skills.go:99`).
- **[ПРОВЕРЕНО]** MCP встроен с 0.99.0 (`CHANGELOG.md:677`): `~/.pi/agent/mcp.json` и `.pi/mcp.json`, формат `mcpServers` как у Claude Code, транспорты stdio и streamable HTTP, SSE отвергается (`docs/mcp.md:32,44-64`). Расширение может добавить сервер на сессию: `pi.registerMcpServer()` (`docs/mcp.md:262`). В SDK MCP подключается явно через `createMcpExtension()` (`docs/sdk.md:116`).
- **[ПРОВЕРЕНО]** Разворот позиции. В 2025 автор писал «pi does not and will not support MCP» (https://mariozechner.at/posts/2025-11-30-pi-coding-agent/). PR #10040 (mitsuhiko, 2026-09-25): «We have mixed feelings about MCP, but some uses of it are responsible». По умолчанию инструменты MCP модели не объявляются, а доступны через `codemode` (`docs/mcp.md:193-200`).
- **[ВЫВОД]** Общий реестр MCP на город для pi реализуем: один HTTP MCP-сервер на город, а pi-сессии получают запись с `url` в сгенерированном `mcp.json` или через `registerMcpServer`. Для stdio-серверов каждый pi запускает свой экземпляр.
- **[ВЫВОД]** Стандартизированные хуки. Общее подмножество отображается так:

| Декларативный хук оркестратора | Событие pi |
|---|---|
| старт сессии | `session_start` |
| перед ходом / ввод пользователя | `before_agent_start`, `input` |
| перед инструментом (может запретить) | `tool_call` |
| после инструмента | `tool_result`, `tool_execution_end` |
| агент остановился | `agent_settled` |
| перед компакцией / после | `session_before_compact` / `session_compact` |
| конец сессии | `session_shutdown` |

  У claude и codex хук — внешняя команда, у pi — код в процессе. Значит, для pi оркестратор поставляет одно сгенерированное расширение, которое вызывает команды. Gas City делает именно это: `gc-hooks.js` (206 строк, версия 10) вешает `session_start`, `session_compact`, `before_agent_start`, `message_end`, `agent_end`, `session_shutdown` и зовёт `gc prime --hook`, `gc hook --inject` (`internal/bootstrap/packs/core/overlay/per-provider/pi/.pi/extensions/gc-hooks.js:11-13,172-203`).
- **[ПРОВЕРЕНО]** У владельца стоит одно расширение — `herdr-agent-state.ts` (`session_start`, `agent_start`, `agent_settled`), и оно само отключается вне TUI (комментарий в строке 230).

### 2.8. Конфигурация

- **[ПРОВЕРЕНО]** Слои: каталог агента (`~/.pi/agent`, переопределяется `PI_CODING_AGENT_DIR` или `agentDir` в SDK) → проектный `.pi/` (после project trust) → флаги CLI (`docs/configuration.md:3-9`). Списки ресурсов объединяются, скалярные настройки проекта перекрывают пользовательские (`docs/settings.md:3`).
- **[ПРОВЕРЕНО]** В неинтерактивных режимах проектные ресурсы без `--approve` или сохранённого решения молча пропускаются (`docs/security.md:75-82`). Оркестратор обязан передавать `--approve` либо держать всё в своём каталоге агента.
- **[ПРОВЕРЕНО]** На сессию задаются: в rpc — флагами процесса (`--model`, `--thinking`, `-e`, `--skill`, `--no-extensions`, `--tools`, `--no-mcp`, `--append-system-prompt`); в SDK — объектами `resourceLoader`, `settingsManager`, `model`, `tools` на каждую сессию (проверено экспериментом).
- **[ПРОВЕРЕНО]** Изоляция от домашнего каталога работает: все прогоны шли с `PI_CODING_AGENT_DIR=lab/pi/agent-dir`. Адаптер `pi-acp` эту переменную учитывает не везде: session-map он пишет в `homedir()/.pi/pi-acp` (`src/acp/paths.ts:10`, issue #167), поэтому запускал его с подменённым `HOME`.

### 2.9. Подписки и политика

- **[ПРОВЕРЕНО]** У владельца `~/.pi/agent/auth.json` — пустой объект (2 байта, `jq keys` → `[]`). `pi --list-models` показывает только провайдеров `google` (23 модели) и `nvidia` (21) — они берутся из `GEMINI_API_KEY` и `NVIDIA_API_KEY`. Подписки Claude/ChatGPT/Copilot в pi не подключены.
- **[ПРОВЕРЕНО]** pi умеет OAuth-подписки (Anthropic, «Sign in with ChatGPT» с 0.99.0, Copilot и др.). Для Anthropic в коде зашито предупреждение: «Third-party harness usage draws from extra usage and is billed per token, not your Claude plan limits» (`src/modes/interactive/interactive-mode.ts:334`; настройка `warnings.anthropicExtraUsage`, `docs/settings.md:173`).
- **[ПРОВЕРЕНО по вебу]** Anthropic с 2026-04-04 перевёл использование подписки из сторонних харнессов на отдельную оплату (https://www.theregister.com/2026/04/06/anthropic_closes_door_on_subscription/).
- **[ВЫВОД]** Гонять Claude через pi по подписке экономически бессмысленно; pi разумен для не-Anthropic моделей. Политику OpenAI для ChatGPT-подписки в pi не проверял.

### 2.10. Как Gas City поддерживает pi сейчас

- **[ПРОВЕРЕНО]** Профиль `pi` (`internal/worker/builtin/profiles.go:892-921`): запуск `pi -e .pi/extensions/gc-hooks.js`, `ReadyDelayMs: 8000`, resume через `--session`, статус по OSC 7501 с принудительным `PI_PROGRAM_STATUS=1` («tmux never does» отвечать на probe). `SupportsACP` у pi нет; в файле он выставлен у claude, kimi, kiro, opencode, mimocode, cerebras, groq.
- **[ПРОВЕРЕНО]** Прерывание хода pi в tmux — «hard restart» (`requiresHardRestartInterrupt`, `internal/session/submit.go:778-783`), и после него не ждут idle (`:785-790`). Для nudge нужен отдельный poller (`cmd/gc/cmd_wait.go:1495`).
- **[ПРОВЕРЕНО]** Свой парсер сессий pi — `internal/sessionlog/pi_reader.go`, 959 строк; pi перечислен в списке особенностей отправки клавиш (`internal/runtime/tmux/tmux.go:54`).
- **[ВЫВОД]** Всё перечисленное — обходные пути вокруг TUI. В rpc это штатные команды и события: `abort`, `agent_settled`, `steer`, `get_messages`.

### 2.11. Зрелость

- **[ПРОВЕРЕНО]** pi: `gh api repos/earendil-works/pi` — 121 096★, 15 600 форков, 271 открытый issue. npm-пакет переименован в `@earendil-works/pi-coding-agent` (создан 2026-05-07), старый `@mariozechner/pi-coding-agent` остановился на 0.73.1. Мейнтейнеры npm: badlogic, mitsuhiko, rwachtler. Коммиты: badlogic 4302, mitsuhiko 984, davidbrai 389, christianklotz 231, cristinaponcela 207.
- **[ПРОВЕРЕНО]** Темп: 25 релизов с 2026-08-06 по 2026-11-09; 1.0.0 — 2026-10-01, затем 1.1–1.5 примерно раз в неделю.
- **[ПРОВЕРЕНО]** `pi-acp`: 134 из примерно 155 коммитов у одного автора, 98 открытых issues и PR, issue #175 (2026-11-04): «the project has stagnated, and for now upstream doesn't want to implement the ACP functionality». Минимальная заявленная версия pi — 0.81.0; на 1.5.0 базовый поток у меня работал.
- **[ПРОВЕРЕНО]** В монорепозитории появился экспериментальный `@earendil-works/pi-durable` («Durable agent harness… If the process dies mid-turn, reopening the storage picks the work up», `src/pi-mono/packages/durable/README.md:3-5`) — много разговоров в одном процессе с возобновлением хода. Помечен «API changes without notice».

## 3. Вердикты по гипотезам (в части pi)

- **H1 — подтверждена, но не через ACP.** tmux для pi не нужен: rpc даёт структурные события, очереди, отмену, ретраи и возобновление сессии — всё проверено. Оговорки: класс ошибки надо выводить из текста; `agent_settled.aborted:false` и exit 0 при провале; после краша ход не продолжается сам; остаются процессы-сироты. Через `pi-acp` 0.0.34 гипотеза не выполняется: гибель pi подвешивает сессию, ошибки маскируются под `end_turn`.
- **H3 — опровергнута для pi в формулировке «все нормально работают через ACP».** Нативного ACP нет, автор отказывает с декабря 2025. Адаптер из реестра годится для одного человека в Zed, но не для оркестратора. `@openma/pi-acp` выглядит полнее, но это проект одного автора с 4 звёздами.
- **H4 — для pi через ACP опровергнута, через SDK подтверждена.** `pi-acp`: 2 процесса и около 188 МБ RSS на сессию, одна живая сессия на соединение. SDK: 4 сессии в одном процессе около 124 МБ. Источник экономии — встраивание, а не протокол.
- **H9 — частично.** Сам pi всё нужное умеет: слоёный конфиг, стандартные skills, встроенный MCP с HTTP-транспортом, 41 событие. Утверждение «хуков в Gas City нет» для pi неточно: управляемое расширение есть, но оно зашито под команды `gc`, а не задаётся пользователем декларативно.
- **H10 — частично.** К живой сессии pi второй клиент подключиться не может ни в rpc, ни в SDK, ни через `pi-acp`. Возможны два пути: человек открывает файл сессии в TUI, когда оркестратор её не держит; либо оркестратор сам даёт человеку интерфейс и транслирует ввод как `prompt`/`steer`.

## 4. Следствия для дизайна и варианты

Сравнение драйверов pi:

| Критерий | tmux + TUI (сейчас) | ACP (`pi-acp`) | `--mode rpc` | SDK в процессе |
|---|---|---|---|---|
| Память на сессию | ≈ процесс pi (**[НЕ ПРОВЕРЕНО]**, TUI не мерил) | ≈ 188 МБ RSS | ≈ 96 МБ RSS | ≈ 2 МБ heap сверх ≈ 140 МБ базы |
| Готовность/idle | OSC 7501, задержки, poller | ответ на `session/prompt` | `agent_settled` | `agent_settled` |
| steer / follow_up | клавиши | только очередь | да | да |
| Отмена | hard restart | `session/cancel` | `abort`, 7 мс | `abort()` |
| Ретраи, квота | парсинг экрана | скрыто, ошибка → `end_turn` | события + текст ошибки | события + текст ошибки |
| Краш харнесса | tmux-пан умер | сессия зависает (#160) | exit процесса, `--session` | падают все сессии процесса |
| MCP на сессию | `mcp.json` | игнорируется | `mcp.json`, флаги | `createMcpExtension` |
| Разный конфиг на сессию | флаги | нет | флаги | объекты на сессию |
| Язык оркестратора | любой | любой | любой | только Node/Bun |
| Риск сопровождения | свой код Gas City | сторонний MVP | официальный, документирован | официальный |

Насколько «ACP как единый транспорт» обедняет pi: теряются steer, структурные ошибки и ретраи, ветвление, UI расширений, MCP на сессию и выигрыш по памяти. Остаётся prompt/cancel/load — этого мало даже для обработки квоты.

Варианты:

1. **Рекомендую: абстракция «драйвер харнесса» с родным бэкендом на каждый харнесс; для pi — `--mode rpc`.** Процесс на сессию, около 96 МБ, изоляция сбоев, любой язык оркестратора, официальный протокол. ACP оставить для харнессов с официальным адаптером. Цена — отдельный драйвер для pi (JSONL-клиент; готовый `RpcClient` есть для TypeScript).
2. **Если важна память при десятках pi-сессий: sidecar на Node с SDK.** Один процесс держит N сессий и отдаёт оркестратору по сессии тот же rpc-словарь. Около 2 МБ на сессию вместо 96, но падение sidecar роняет все сессии сразу (они восстановимы из JSONL), и появляется свой код-хост.
3. **ACP для pi через `@openma/pi-acp` или собственный адаптер на SDK** — только если единый транспорт ценнее функций. Это ставка на проект одного автора; требует аудита кода и принятия на сопровождение.

Что оркестратор обязан делать сам при любом варианте: классифицировать `errorMessage` (квота, auth, лимит); считать ход проваленным по `stopReason:"error"` последнего сообщения, а не по `agent_settled`; после краша слать явный prompt «продолжай»; убивать группу процессов; передавать `--approve` или держать ресурсы в своём каталоге агента; не открывать один файл сессии двумя процессами.

## 5. Открытые вопросы и непроверенное

- TUI-подключение человека к сессии, созданной в rpc, — выведено из общего кода, интерактивно не проверял.
- `@openma/pi-acp` не запускал; его мультисессионность и память — только по коду и README.
- Реальные ответы провайдеров-подписок на исчерпание лимита (Claude, ChatGPT, Copilot) не воспроизводил: у владельца подписки в pi не подключены. Mock показывает механику, не формулировки.
- Настоящую компакцию при переполнении контекста и поведение при `length` не воспроизводил.
- Память SDK и rpc на длинных сессиях (сотни ходов) не мерил.
- Поведение двух процессов на одном файле сессии не проверял — установлено только отсутствие блокировок.
- Политика OpenAI по использованию ChatGPT-подписки из pi — не проверена.
- `pi-durable` не пробовал.

### Отступления от правил и побочные данные

- **Побочный эффект в `~/.pi`.** Один мой ранний вызов `pi` без `PI_CODING_AGENT_DIR` создал два пустых каталога: `~/.pi/agent/sessions/` и вложенный в него каталог для `lab/pi`. Я удалил оба через `rmdir` (срабатывает только на пустых); состав `~/.pi/agent` снова тот же, что до начала — `auth.json`, `extensions/`, `models-store.json`. Дальше все запуски шли с изолированным каталогом агента.
- **Чтение кода `pi-acp` перед запуском было частичным.** До запуска я прочитал spawn, пути, работу с окружением и обработчики `initialize`/`newSession`/`loadSession`/`cancel`; grep по сетевым и exec-вызовам во всём `src/` сделал уже после (нашлись только `spawn` pi, запись session-map и `unlinkSync` файла сессии при удалении). Все 4,4 тыс. строк не читал.
- **codex-acp (по первому сообщению оркестратора, позже отозванному).** Данные уже были собраны, оставляю кратко. `src/codex-acp` — архивный `zed-industries/codex-acp` (`archived:true`, последний push 2026-07-22). Актуальный — `agentclientprotocol/codex-acp` 2.6.0 (он же в реестре ACP). Он стартует один `codex app-server` на весь адаптер (`src/codex-acp-current/src/index.ts:68-71`), сессии — потоки внутри него. Замер без запросов к LLM, с пустым `CODEX_HOME` в lab и фиктивным API-ключом (`scripts/x1-codex-acp-rss.mjs`): 0/1/2/4 сессии = 200 / 248 / 255 / 264 МБ RSS суммарно (node ≈ 105 + codex 96→159). Без авторизации `session/new` отвечает `-32000 Authentication required`. Ходы не выполнялись, цифры — нижняя граница.

# 44 — «А если просто N агентов на штатном Claude Code?» Что он закрывает без своего оркестратора, а что нет

Дата: 2026-11-11. Проверялся `claude` 2.1.322; `claude --help` совпадает с копией от 10.11 (`LAB/logs/claude-help.txt`).
Документация скачана 11.11 в `LAB/web/` (сутки назад `workflows.md` был другим, см. §2). Опыты E1–E5 шли на haiku:
9 запусков `claude -p` с ходами модели и 6 простаивающих без ходов, 14 субагентов и workflow-агентов, условная стоимость
≈ $0,03. Опыты — в отдельном git-репозитории `LAB/repo`.

Сокращения: `LAB` = `tmp/gc-alt-research/lab/cc-native`, `WEB` = `LAB/web`, «отчёт NN» = `reports/NN-*.md`.
Метки: **[П]** проверено (опыт, док, `--help`), **[В]** вывод, **[НП]** не проверено.

## 1. Выводы за 60 секунд

**Чем «N агентов на штатном Claude Code + workflows» отличается от сетапа.**

- **Граф одного прогона из Claude-ролей штатный Claude Code уже даёт почти целиком, в том числе без интерфейса.**
  Workflow — это JS-скрипт, в котором:
  - `agent(prompt, {agentType})` выбирает роль для шага;
  - `await` задаёт зависимости, а `if`/`while` — условия и check-петли;
  - `parallel()` и `pipeline()` дают fan-out;
  - `isolation: 'worktree'` — worktree на агента, `schema` — проверенный JSON-результат;
  - зависший агент перезапускается сам (до 5 раз), прогон возобновляется по журналу.

  Опыт E2: saved workflow отработал из `claude -p`, ролей было две, параллельных агентов три, check-петля прошла за 2 попытки.
  **[П]**
- **Разница в том, кто владеет графом.** В штатном пути граф — это эфемерная переменная скрипта внутри процесса одной
  сессии Claude. В Gas City или своём оркестраторе граф живёт в трекере и переживает процессы. Из этого вытекает всё, чего нет
  штатно:
  1. Трекера нет: ни бидов, ни эпиков над ригами, ни claim, ни KV.
  2. Нет шагов-скриптов без LLM: у скрипта workflow нет shell (`WEB/workflows.md:353`), скрипт запускает только LLM-агент.
     В опыте E3 такой агент вернул ожидаемый текст, не запустив команду.
  3. Нет human gate внутри прогона (`:352`).
  4. Нет самовосстановления: после `kill -9` прогон стоит, пока кто-то не сделает `--resume` той же сессии и не перезапустит его.
     Прерванный шаг повторяется с нуля.
  5. В `-p`, `--bg` и SDK нет ожидания сброса лимита (`:405`).
  6. Нет других харнессов: «In every approach the workers are Claude sessions» (`WEB/agents.md:19`). codex и pi остаются
     Bash-командой внутри Claude-агента.
  7. Нет локальных ордеров без открытой сессии.

  **[П]**
- **Хуки на агента — да, проверено опытом.** Две роли из `--agents` с разными хуками и разными MCP в одном `claude -p`:
  - хук роли A срабатывал только в A, хук роли B — только в B;
  - MCP-сервер роли поднимался при старте её агента и гас после него;
  - `disallowedTools: ["mcp__mcpP"]` отрезал роли B общий MCP родителя (E1, E2).

  **[П]** Пять оговорок:
  1. Хуки сессии (`--settings`) срабатывают и внутри каждого субагента, с `agent_type` и `agent_id` во входе. Отключить их для
     роли нельзя, можно только фильтровать в самом хуке. **[П]**
  2. У агента из проектного `.claude/agents/` в недоверенном каталоге хуки молча пропускаются, а агент всё равно работает.
     В debug-логе: `Skipping frontmatter hooks for agent 'roleC': … not trusted` (E5). **[П]**
  3. У агентов из плагина поля `hooks`, `mcpServers` и `permissionMode` игнорируются (`WEB/sub-agents.md:234`). **[П]**
  4. Тиммейтам agent teams хуки из определения не передаются (`WEB/agent-teams.md:274-283`). **[П]**
  5. Поля `env` у субагента нет. `permissionMode` роли игнорируется, если родитель работает в `bypassPermissions`, `auto`
     или `acceptEdits` (`WEB/sub-agents.md:594`). Env, `--settings` и `--setting-sources` на роль даёт только отдельный
     процесс (`-p`/`--bg`). **[П]**
- **Ресурсы.** Три параллельных workflow-агента в одном процессе добавили около 50 МБ RSS к ~240 МБ базы, включая shell и MCP.
  Три отдельных `claude -p` в простое занимают 610–705 МБ RSS (430–490 МБ PSS) (E2, E4). **[П]**
- **Самое неожиданное — «успешный провал» внутри workflow.** Агент шага C (роль A, haiku) ответил `k-C`, не вызвав Bash.
  В marks нет `PreToolUse`, в журнале нет `toolsUsed`, а прогон засчитал шаг выполненным (E3). Workflow верит тексту агента,
  поэтому проверка результата по состоянию всё равно лежит на владельце. **[П]**
- **Ответ владельцу.** Для прогонов под присмотром — сидишь в интерактивной сессии, `ultracode`/saved workflows, br через
  CLI — штатного хватает уже сегодня. Здесь даже работает ожидание лимита, которого нет в headless.

  Для флота без присмотра (бид → формула → роли → проверки → gate → следующий бид, переживает краш и лимит, есть codex-шаг)
  штатный Claude Code закрывает граф **внутри прогона**, но не **между прогонами**. Тонкий диспетчер всё равно нужен, но
  меньше C3: ≈0,9–1,9 тыс. строк, из них своего Python 0,6–1,2 тыс., против 1,5–3 тыс. (§6). **[В]**
- **Риск — дрейф.** За сутки в `workflows.md` поменялась семантика resume после лимита (v2.1.318) и правила паузы (`diff`, §2).
  Agent view и cloud scheduled tasks — research preview, agent teams — experimental. Всё это расходует подписку. **[П]**

## 2. Карта штатных механизмов

| Механизм | Что даёт для оркестрации | Где живёт | Настройка на роль | Статус |
|---|---|---|---|---|
| **Subagents** (`.claude/agents/*.md`, `~/.claude/agents`, `--agents` JSON или файл в `-p`) | делегирование; до 20 одновременно, вложенность до 3 (`WEB/sub-agents.md:1061,1088`); resume через `SendMessage` | **внутри процесса** сессии | model, effort, tools/disallowedTools, permissionMode*, mcpServers, hooks, skills, memory, isolation, maxTurns, omitClaudeMd; **env нет** (`:296-316`) | GA |
| **Dynamic workflows** (`.claude/workflows/*.js`, плагин `workflows/`, или скрипт, написанный Claude) | JS: `agent/parallel/pipeline/phase/log`, `workflow()` на 1 уровень вложенности; `agentType`, `model`, `effort`, `isolation`, `schema`, `stallMs`; до 16 агентов параллельно, максимум 1000 на прогон (`:355-358`) | внутри процесса сессии; журнал — `~/.claude/projects/<proj>/<sid>/subagents/workflows/<run>/journal.jsonl` | через `agentType` — всё, что у субагента | метки preview на странице нет, но меняется каждую неделю |
| **Agent teams** | lead + тиммейты, общий task list с claim через file lock, mailbox-файлы | только **интерактив**, в `-p` тиммейты не порождаются (`WEB/agent-teams.md:60`) | tools, model, effort, тело; MCP — только в split-pane; **hooks и skills не применяются** | experimental, выключено по умолчанию |
| **Agent view / `--bg` / supervisor** | фоновые сессии, `claude agents --json`, `attach`, `logs`, `stop`, `respawn`, авто-рестарт упавшего процесса | **процесс на сессию** (`WEB/agent-view.md:856`); демон общий с сессиями человека | всё: `--settings`, `--mcp-config`, `--agent`, env | research preview (`:1043`) |
| **`claude -p` / Agent SDK** | stream-json: `api_retry`, `rate_limit_event`, `result`, события хуков; `--resume`; `--json-schema` | процесс на запуск | всё, на процесс | GA; в будущем `--bare` станет режимом `-p` по умолчанию (`WEB/headless.md`) |
| **Cross-session messaging** | `ListAgents`/`SendMessage` между сессиями; inbox-сокет есть и у `-p` | сокет на сессию | `crossSessionInbound` в `--settings` | GA; в `-p` удержанное сообщение через 5 мин отбрасывается (`WEB/cross-session-messaging.md:230-237`) |
| **`/loop`, Cron-инструменты** | повтор промпта по интервалу | **session-scoped**: нужна живая сессия; recurring сам удаляется через 7 дней (`WEB/scheduled-tasks.md:28,209`) | — | GA |
| **Desktop scheduled tasks** | локальный cron | только пока **открыт Desktop-app** | — | GA |
| **Cloud scheduled tasks (routines)** | cron, API POST, GitHub-события | **облако, свежий клон**, без локальных файлов; 100 прогонов в час | коннекторы на задачу | research preview (`WEB/routines.md:11`) |
| **`/goal`, Stop/SubagentStop-хук** | «работай, пока условие не выполнено»; хук с exit 2 не даёт агенту закончить и возвращает ему фидбек | в сессии | Stop-хук в определении роли | GA |
| **Plugins** | упаковка «пака»: skills, commands, agents, hooks, MCP, workflows, `bin/`, userConfig | на сессию | хуки и MCP плагина действуют на **всю** сессию, а не на роль; из settings берутся только `agent` и `subagentStatusLine` (`WEB/plugins_components.md:181`) | GA |
| **Worktrees** | `-w`, `isolation: worktree` у субагента и шага workflow; хуки `WorktreeCreate/Remove` | репо/`.claude/worktrees` | на роль | GA |
| **Remote Control, Projects, Channels, Mods** | пульт с телефона, облачные треды, push событий, панели и таймеры | интерактив/облако | — | Channels — preview; Projects — beta |

\* `permissionMode` роли действует только при родителе в `default`/`dontAsk`.

**Дрейф за сутки** **[П]**: `diff lab/factcheck/web/workflows.md LAB/web/workflows.md` даёт новое правило resume «Failed at
a usage limit on v2.1.318 or later» и описание паузы по лимиту для Desktop Code tab.

## 3. 14 must-требований владельца × штатный Claude Code

Источник требований — отчёт 02 §4. «Внутри прогона» значит в пределах одного workflow одной сессии.

| № | Требование | Штатно | Чем закрыть |
|---|---|---|---|
| 1 | Риги, один эпик над бидами нескольких ригов | **нет** | br (одна БД, `rig:` labels); `--add-dir` даёт только доступ к файлам |
| 2 | Бид-стор: типы, `blocks`, KV, `ready` | **нет**. Task list есть только у agent teams (интерактив) и в `TodoWrite` агента | br + своя SQLite под KV (отчёты 11, 35) |
| 3 | Роли: харнесс, модель, effort, env, права, fresh/resume, pool | **частично**. Внутри процесса — model, effort, tools, MCP, hooks, skills; env нет; харнесс только claude; pool — общие лимиты (20 субагентов, ≤16 агентов workflow) | env и settings — отдельным процессом на роль; codex и pi — внешний драйвер |
| 4 | Скриптовые агенты без LLM | **нет**. Скрипт workflow без shell; шаг-скрипт = LLM-агент (≈ $0,001 и 2–3 с на haiku, риск выдуманного результата, E3) | исполнять exec-шаги в диспетчере или в Stop/SubagentStop-хуке роли |
| 5 | Claim: атомарный захват, возврат при смерти | **частично**. Внутри прогона не нужен (раздаёт скрипт); между прогонами нет; claim в teams — только интерактив | `br update --claim` в диспетчере |
| 6 | Маршрут на роль, зависимости, условие, check-петля, fan-out, abort/cleanup | **да внутри прогона** (`agentType`, `await`, JS `if`/`while`/`try`, `parallel`/`pipeline`). Условие pass/fail — из `schema`-ответа LLM, не из exit code | exit code — через диспетчер или хук |
| 7 | Gates: human и внешнее условие, с таймаутом | **нет** (`WEB/workflows.md:352`: «For sign-off between stages, run each stage as its own workflow») | граница прогона = gate; бид `gate:human` в br |
| 8 | Worktree на задачу, сессия стартует в нём | **да** (`isolation: 'worktree'`, `-w`) | очистка изменённых worktree — своя |
| 9 | Pack-расширения: команды, шаблоны, vars рига | **частично**. Плагин: skills, commands, workflows, `bin/`, userConfig, `args` у workflow. Хуки и MCP плагина — на всю сессию, у агентов плагина без hooks/MCP | роли — `--agents` JSON рядом с паком |
| 10 | Хуки один раз на роль → claude/codex/pi | **частично**. Для claude — да (E1, E2, с оговорками §1); трансляции на codex и pi нет | отчёт 18 |
| 11 | Общий MCP + opt-in/opt-out по роли | **да**. В одном процессе: `mcpServers` в роли, строковая ссылка разделяет соединение родителя, `disallowedTools: mcp__X`. Между процессами — общий HTTP-MCP (отчёт 36) | — |
| 12 | Liveness по протоколу; «ждёт лимита / логина / человека» | **частично**. Workflow перезапускает зависшего агента ≤5 раз (`WEB/workflows.md:421`); у agent view есть `Needs input`; в `-p` есть `api_retry`, `rate_limit_event` и хук `StopFailure{error}`. Пауза по лимиту — только в интерактиве | парковка до `resetsAt` и relaunch — в диспетчере |
| 13 | Событийный хоп ≤ 5 с | **да внутри прогона**: `SubagentStop` → следующий `SubagentStart` ≤ 1 с (E2). Между прогонами — нет | диспетчер по exit процесса |
| 14 | Сессия оператора (attach/resume), почта с dedup | **частично**: `claude agents`, `attach`, `/workflows`, уведомления `agent_needs_input/agent_completed`; почты с dedup нет | бид `to:human` |

**Итог** **[В]**:
- полностью — 4 (6, 8, 11, 13), причём 6 и 13 только внутри прогона;
- частично — 6 (3, 5, 9, 10, 12, 14);
- нет — 4 (1, 2, 4, 7).

У C3 (отчёт 33 §4.3) было 7 полностью и 7 частично. Штатный путь силён ровно там, где C3 слаб (граф шагов), и слаб там,
где C3 силён (трекер, exec, процессы).

**Формулы servitors (отчёт 35 §1.2) на штатном пути** **[В]**:
- Ложатся без натяжек: `explore`, `plan` без gate и `implement` (`pipeline` по задачам с worktree).
- `plan-review` и `qa` — check-петля ложится, но шаги-проверки и `tester run/gather` (скрипты) становятся LLM-шагами.
- `plan-crossreview` — codex-ревьюер превращается в Claude-обёртку вокруг `codex exec`, типизированные ошибки codex
  теряются.
- Все `approve` — это граница прогона.

## 4. Опыты

Общая форма запуска (`LAB/scripts/run.sh`):

```
env -i HOME PATH … CLAUDE_CODE_DISABLE_CLAUDE_MDS=1 CLAUDE_CODE_DISABLE_AUTO_MEMORY=1 \
  claude -p --model haiku --effort low --setting-sources "$SRC" \
  --settings cfg/<t>-settings.json --mcp-config cfg/<t>-mcp.json --strict-mcp-config \
  --agents cfg/<t>-agents.json --permission-mode dontAsk \
  --output-format stream-json --verbose --include-hook-events "<prompt>"
```

`cwd` = `LAB/repo`, конфиги — `LAB/scripts/gen_cfg.py`. В `--settings` хуки `PARENT`; в `--agents` роли `roleA`/`roleB`
со своими хуками `ROLE-A`/`ROLE-B` и inline stdio-MCP `mcpA`/`mcpB` (у B ещё `disallowedTools: ["mcp__mcpP"]`); общий MCP
родителя — `mcpP`. `hook.sh` пишет `event, agent_type, agent_id, tool` в `logs/<t>-marks.tsv`, `mcp_stub.py` — start/call
с pid/ppid в `logs/<t>-mcp.log`, `sample.py` раз в секунду снимает память дерева по `/proc/*/smaps_rollup`.

**E1 — два субагента с разными хуками и MCP в одном `-p`** (`logs/e1-*`). Родитель по очереди делегирует A и B, каждый
делает `echo` и пробует `ping` у `mcpA`, `mcpB`, `mcpP`. **[П]**
- **Хуки.** Хук `ROLE-A` сработал только на вызовах агента `a0a72664…` (roleA), `ROLE-B` — только у `a2cc1ce8…` (roleB).
  `PARENT` сработал на каждом вызове обоих субагентов, с `agent_type`, и на вызовах самого родителя. `Stop` в определении
  роли пришёл как `SubagentStop`.
- **MCP.** Процесс `mcpA` стартовал в 04:49:06 вместе с roleA, `mcpB` — в 04:49:11 вместе с roleB. Все — дети одного
  процесса `claude` (ppid 110296). roleA видел `mcpA` и `mcpP`, но не `mcpB`. roleB видел только `mcpB`: `mcpP` отрезан
  `disallowedTools`. Подтверждено строками `call ping` в логе заглушек.
- **Цифры.** Дерево — 241–298 МБ RSS, один процесс `claude`. 16 с, $0,0066.

**E2 — saved workflow из `claude -p`** (`repo/.claude/workflows/lab-graph.js`, `logs/e2*`). **[П]**
- **С `--setting-sources ""` проектные workflows не грузятся.** Ответ инструмента: `Workflow "lab-graph" not found.
  Available: deep-research`. С `--setting-sources project` прогон пошёл. Ключевое слово `ultracode` в `-p` не работает
  (`WEB/workflows.md:131`), но прямая просьба «use the Workflow tool to run lab-graph» сработала: модель вызвала
  `Workflow{name}`.
- **Ход прогона.** Шаг A (roleA), затем `parallel` из трёх roleB с `sleep 6`, затем check-петля: roleA запускает
  `sh ./check.sh`, скрипт падает на первой попытке и проходит на второй, `schema {pass, output}`.
- **Результат:** `{"a":"wf-A","fan":["wf-B-x","wf-B-y","wf-B-z"],"attempts":2,"verdict":{"pass":true,"output":"PASS attempt 2"}}`.
  7 агентов за 23 с. Хоп между агентами ≤ 1 с (`SubagentStop` 04:50:16 → `SubagentStart` 04:50:17).
- **Хуки ролей работают и в workflow-агентах с `agentType`.** Три параллельных roleB обслужил **один** процесс `mcpB`, а
  `mcpA` поднимался заново на каждый последовательный агент roleA.
- **Процессы.** Всё время один процесс `claude`. Пик дерева — 9 процессов (claude, 2 заглушки MCP, 3 zsh, 3 sleep),
  329 МБ RSS / 271 МБ PSS против 242/216 до fan-out.
- Workflow запустился «in background», `-p` дождался конца и сделал ещё ход с ответом. Bash агентов шёл в zsh владельца
  через `~/.claude/shell-snapshots/snapshot-zsh-*.sh`.

**E3 — `kill -9` посреди прогона и resume** (`repo/.claude/workflows/lab-kill.js`: A → B (`sleep 20`) → C;
`logs/e3*`, `logs/e3-journal.jsonl`). **[П]**
- **Убийство.** В 04:51:14, когда B был в `sleep 20`, `claude -p` получил `kill -9` (rc=137).
- **После убийства.** stdio-MCP завершились по EOF. Bash-потомок `zsh -c … sleep 20` **осиротел** (ppid=1, своя pgid) и
  доработал. Остался устаревший inbox-сокет `/tmp/cc-socks/115853.sock` (я его удалил). В журнале — `result` A и
  `started` B. Само ничего не перезапустилось: супервизора у `-p` нет.
- **Resume** (`run.sh … --resume 93071299-…`, промпт «relaunch with scriptPath=… resumeFromRunId=wf_c935369a-585»;
  27 с, ≈ $0,002). A вернулся из кэша (нового `SubagentStart roleA` перед B нет). **B прогнан заново с нуля** — побочные
  эффекты шага повторяются. Затем C; итог `{"a":"k-A","b":"k-B-end","c":"k-C"}`. Журнал лежит в каталоге сессии, так что
  без `--resume` той же сессии relaunch невозможен.
- **«Успешный провал».** Агент C (`a3c24ec3…`) вернул `k-C`, **не вызвав Bash**: в marks у него только
  `SubagentStart`/`SubagentStop`, в журнале нет `toolsUsed`, в транскрипте один текстовый блок.

**E4 — память: отдельные процессы против агентов в одном процессе** (`logs/e4*`). **[П]**
- Три одновременных `claude -p` с одним Bash-ходом: 10,8–11,0 с каждый.
- Три простаивающих `claude -p --input-format stream-json`, без хода модели, каждый со своим stdio-MCP:
  611–705 МБ RSS / 427–489 МБ PSS на всех (`logs/e4idle-sample.tsv`), то есть около 140–160 МБ PSS на процесс.
  Это сходится с отчётами 14 и 42 (≈156–236 МБ на процесс).
- Для сравнения в E2: +3 workflow-агента ≈ +50 МБ RSS / +55 МБ PSS вместе с их shell и одной заглушкой MCP, то есть
  ≈ 15–20 МБ на агента.

**E5 — роль в `repo/.claude/agents/roleC.md` (проект), `-p` без принятого доверия** (`logs/e5*`, `logs/e5-debug.txt`). **[П]**
- Агент **работал**, но в debug-логе две строки:
  - `[ERROR] Skipping frontmatter hooks for agent 'roleC': the folder its definition file came from is not trusted`;
  - `[WARN] Skipping frontmatter MCP servers: blocked by --strict-mcp-config (agent source: projectSettings)`.
- В marks нет ни одной строки `ROLE-C`, процесс `mcpC` не стартовал.
- Для сравнения: inline-MCP из `--agents` флаг `--strict-mcp-config` не фильтрует (`WEB/sub-agents.md:573`).
- Вывод: защитные хуки роли (аналог `deny-unbounded`) при неверном способе доставки молча не действуют.

**Не ставились:** agent teams (только интерактив), `--bg` (проверен в отчёте 36), настоящий лимит или квота, `attach`/TUI.

## 5. Где штатный путь лучше своего оркестратора, а где хуже

**Лучше** **[П]/[В]**:
- **Граф внутри прогона бесплатен.** Петли, ветвления, fan-out, worktree, structured output и stall-restart — это 20–100
  строк JS на формулу, без своего движка. Резюме по журналу работает (E3).
- **Роли дёшевы.** Все агенты в одном процессе: ~15–20 МБ на параллельного агента против ~150–230 МБ на процесс. Кэш
  промпта общий между сиблингами (`WEB/workflows.md:340-344`).
- **Хуки и MCP на роль — нативно**, в одном формате с хуками владельца (`deny-unbounded`, `stop-gate`, `replan` переносятся
  в определение роли как есть).
- **Пульт готов** (`/workflows`, `claude agents`, `attach`, уведомления, Remote Control), а в интерактиве сессия и workflow
  сами ждут сброса лимита (`WEB/interactive-mode.md:741-760`). Для прогонов под присмотром своего кода — ноль.

**Хуже** **[П]/[В]**:
1. **Граф не переживает процесс.** Краш, закрытый терминал или «Exit and stop tasks» останавливают прогон
   (`WEB/workflows.md:387-391`). Relaunch делает только Claude в той же сессии, прерванные шаги повторяются целиком (E3).
   Нужен внешний «кто-то», кто заметит смерть и перезапустит.
2. **Один процесс на все роли** — общий радиус поражения. `kill -9` или OOM убивает весь fan-out, Bash-потомки остаются
   сиротами (E3).
3. **Шаги-скрипты становятся LLM-шагами.** Цена — ход модели, точность — на совести модели (E3, шаг C). У владельца
   143 из 458 стартов и 26 из 49 шагов формул — скрипты (отчёты 02, 35).
4. **Нет gate и ввода посреди прогона.** Формула режется на прогоны по каждому `approve`.
5. **Лимит в headless не ждёт.** В `-p`, SDK, `--bg`, Remote Control и у тиммейтов агент падает (`WEB/workflows.md:405`,
   `WEB/interactive-mode.md:790`); с v2.1.318 relaunch повторяет только упавших.
   Ирония: автоожидание без человека есть только у интерактивной сессии в терминале — то есть снова tmux.
6. **Только Claude.** codex и pi — через Bash-обёртку, без их типизированных ошибок и `resetsAt`.
7. **Ловушки конфигурации:**
   - хуки сессии наследуются всеми ролями;
   - `permissionMode` роли перекрывается режимом родителя;
   - проектные роли без доверия молча теряют хуки и MCP (E5);
   - роли из плагина теряют хуки, MCP и права;
   - нужен `--setting-sources project`, иначе saved workflows не видны (E2).
8. **Нестабильность API**: десятки поведенческих правок за v2.1.200–2.1.318 (`WEB/agent-view.md:1061-1133`).

## 6. Минимальный сетап, если брать штатный путь

**Вариант 0 — под присмотром, ноль кода** **[В]**:
- интерактивная сессия `claude` в каталоге города, роли в `~/.claude/agents` или доверенном `.claude/agents`;
- формулы — saved workflows, br через CLI, человек — диспетчер и gate.

Работают автоожидание лимита и пульт. Не работают ночные прогоны, краш-восстановление и codex-шаги.

**Вариант 1 — без присмотра: «C3, где граф шагов отдан workflow»** **[В]**:

| Часть | Что делает | Оценка строк |
|---|---|---|
| `roles.json` (`--agents`) | 9 claude-ролей servitors: model, effort, tools/deny, промпт, skills, `hooks` (готовые скрипты владельца), `mcpServers` (kb по HTTP) | 150–250 JSON, генерируется из пака |
| `formulas/*.js` (`.claude/workflows/` пака или плагина) | 7 формул: `agentType` на шаг, `while` для петель, `pipeline` + `isolation` для implement; без gate и exec внутри | 300–600 JS |
| диспетчер (Python) | `br ready` → `br update --claim` → `git worktree add` → `claude -p --setting-sources project --agents roles.json --settings city.json --output-format stream-json "Run workflow X with args {bead}"` в своей pgid | 400–800 |
| ↳ запуск | в своей pgid; `kill` группы при stall | (входит) |
| ↳ лимит | `rate_limit_event.resetsAt` → парковка бида | (входит) |
| ↳ краш | exit ≠ 0 без `result` → `--resume <sid>` + relaunch с `resumeFromRunId` ≤ N раз | (входит) |
| ↳ gate | конец прогона на gate → бид `gate:human` | (входит) |
| ↳ exec-шаги и финишеры | lint/test/merge, `br close` и комментарии, KV в SQLite | (входит) |
| codex/pi-шаги | отдельный драйвер `codex exec --json` в диспетчере (не через Claude-обёртку) | +150–300 |

**Итого ≈ 0,9–1,9 тыс. строк, из них своего Python ≈ 0,6–1,2 тыс.** Это против 1,5–3 тыс. у C3 (отчёт 33 §4.2) и 20–32 тыс.
у варианта A.

**Kill-критерий** — тот же, что в FINAL §5.3 «вендорские workflows покрывают ≥ 80 % формул». По §3 штатно покрываются
маршрут, петли, fan-out и worktree, но не gate, exec и codex. Для servitors это около 60–70 % **конструкций** и ≈ 50 %
**шагов**. **[В]**

**Правила, без которых вариант 1 врёт** **[В]**:
- (а) «сделано» проверяет диспетчер по состоянию репо и бида, а не по ответу workflow (E3, шаг C);
- (б) роли доставлять через `--agents` или `~/.claude/agents`, а не через проектный каталог без доверия (E5);
- (в) хуки сессии писать с учётом `agent_type`;
- (г) отдельная pgid и `kill` группы, иначе сироты (E3);
- (д) пин версии `claude` и контрактный тест «workflow + resume» на каждое обновление.

## 7. Восемь вопросов владельца — коротко

1. **Хуки и прочее на агента.** Субагент или шаг workflow — hooks, MCP, model, effort, tools, prompt, skills. Тиммейт —
   без hooks и skills. Отдельный процесс — всё, включая env. Хуки сессии наследуются всегда (§1, E1). **[П]**
2. **Граф.** Маршрут, зависимости, условие, петля, fan-out (≤16, на 9 CPU меньше; `CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS`
   до 256), `workflow()` на один уровень — есть. Gate и exec-шага нет. Язык — JS в `.claude/workflows/`, `Date.now` запрещён
   ради replay. Resume — только в той же сессии; закрытый терминал останавливает прогон. **[П]**
3. **Трекер.** Бидов нет. Есть `TodoWrite`, task list у agent teams (`~/.claude/tasks/<team>`, только интерактив) и
   переменные скрипта. br — как CLI в allow-правилах; claim, KV, риги и ordering остаются своими. **[П]/[В]**
4. **Сбои.** **[П]**
   - 429 — встроенный retry, события `api_retry` и `StopFailure{rate_limit}`.
   - Лимит — пауза только в интерактиве.
   - Зависание — ≤5 рестартов, окно 10 мин.
   - Краш — прогон умирает; `--bg`-демон поднимает процесс, но не прогон.
   - «Успешный провал» не ловится (E3).
5. **Пульт.** `/workflows` — в интерактивной сессии прогона. `claude agents --json` и `attach` — для `--bg` и интерактива.
   Для `-p` — stream-json и хуки; вмешаться можно `SendMessage` (`crossSessionInbound: accept`) или kill + relaunch. **[П]/[В]**
6. **Ресурсы.** ~15–20 МБ на агента в процессе против ~140–230 МБ PSS на отдельный `claude` (E4). **[П]**
7. **Расписания.** `/loop` — в живой сессии, до 7 дней. Desktop — пока открыт app. Cloud — свежий клон, подписка,
   100 прогонов в час (`WEB/routines.md:462-468`). Локального cron без открытой сессии нет. **[П]**
8. **Политика.** Workflows, `--bg` и cloud-задачи расходуют подписку (`WEB/agent-view.md:1045`, `WEB/routines.md:462`),
   `-p` и SDK — тоже, «for now» (отчёт 22 §3.1). Отложенный отдельный кредит касался именно headless (`-p`/SDK); workflow
   наследует категорию сессии, в которой запущен. **[П]/[В]**

## 8. Побочные эффекты на машине

- `~/.claude/settings.json` не изменился: sha256 `308a0135…a75b3b` и mtime 2026-11-10 19:38 одинаковы до и после
  (`LAB/logs/baseline-{before,after}.txt`). **[П]**
- Записи о `LAB/repo` в `~/.claude.json` нет ни до, ни после (`jq '.projects[...]'` → `absent`). mtime файла меняется от
  других сессий. **[П]**
- Создан `~/.claude/projects/-home-ubuntu-…-lab-cc-native-repo/`, 2,8 МБ: 8 транскриптов сессий, 15 транскриптов агентов
  и 2 каталога workflow-прогонов.
  Журналы скопированы в `LAB/logs/e2b-journal.jsonl` и `e3-journal.jsonl`. Удалить всё можно командой
  `claude purge <LAB/repo>`. **[П]**
- `~/.claude/jobs`, `tasks`, `teams`, `agents` и `workflows` не тронуты. Демон и `--bg` не использовались. **[П]**
- Устаревший сокет своей убитой сессии `/tmp/cc-socks/115853.sock` удалён. Своих процессов не осталось (`ps` по
  `cc-native|mcp_stub|sample.py` пуст). Осиротевший `sleep 20` из E3 завершился сам. **[П]**
- В `LAB/repo` первый коммит прогнал глобальные pre-commit-проверки git: все «Skipped», без изменений. Внутри — `check.sh`,
  `.check-count`, `.claude/{workflows,agents}`. **[П]**

## 9. Не проверено

- **Настоящий лимит подписки** в workflow, в интерактиве и в `-p`, и resume «failed at usage limit» v2.1.318. **[НП]**
- **Agent teams вживую**: только интерактив; применение хуков к тиммейтам взято из доки. **[НП]**
- **Workflow внутри `--bg`-сессии** и его поведение при рестарте процесса демоном (по доке не продолжается сам). **[НП]**
- **Длинные прогоны**: stall-restart, 16 и больше агентов, `isolation: 'worktree'` у шага workflow. Поле видели в API, но
  не запускали. **[НП]**
- **Хуки тиммейтов и in-process teammates** с `agent_id`. **[НП]**
- **Доля «выдуманных» exec-шагов на haiku и sonnet**: в опытах один случай примерно на 13 поручений «запусти команду»,
  статистики нет. **[НП]**
- **Сделает ли Anthropic** workflows/agent view GA или изменит правила подписки. **[НП]**

# 08 — Gas City: конфиг, провайдеры-харнессы, harness hooks, MCP, skills, prompts

Объект: `/home/ubuntu/proj/work/gascity` @ f1f75f53c; живой город `/home/ubuntu/proj/share/gc` (gc `1.5.1-edge.a86d05bd7`);
паки владельца `/home/ubuntu/proj/work/gascity-packs`; клоны `src/pi-mono` @815541a, `src/codex` @aa39ded (оба 2026-11-10).
Пути без префикса — относительно корня gascity. Метки: **[П]** = ПРОВЕРЕНО, **[В]** = ВЫВОД, **[Н]** = НЕ ПРОВЕРЕНО.

## 1. Выводы за 60 секунд

- **«Хуков в gascity нет» — буквально неверно, по сути верно.** gc *сам ставит* harness hooks в 14 харнессов, но только
  свои (4 события: session start / pre-compact / prompt submit / before-agent → `gc prime`, `gc handoff`, `gc nudge drain`,
  `gc mail check`). Декларативной кросс-харнессной модели *пользовательских* хуков нет; событий инструментов
  (`PreToolUse`/`PostToolUse`/`Stop`) в словаре gc нет вообще.
- Владелец уже написал недостающее сам: диспетчер `hooks.d/*.toml` (event/roles/providers/tools/run) на **4302 строки**
  Python/TOML, подключённый в Claude через городской `.claude/settings.json`, а в Codex — через 6 строк
  `-c hooks.<Event>=[…]` в `args_append`. Для pi — ничего.
- **«Нет общего реестра MCP» — наполовину неверно.** Каталог MCP есть (`mcp/<name>.toml` в паке, слои city/rig/import/agent,
  проекция в нативные файлы 6 харнессов, in-band для ACP). Нет другого: **общего процесса**. gc не запускает MCP-серверы и
  не знает об их жизненном цикле; stdio-сервер поднимается на каждый процесс харнесса.
- Общий HTTP-MCP на город *собирается руками* из двух несвязанных фич (`[[service]] kind="proxy_process"` + `mcp/kb.template.toml`
  с захардкоженным портом и `${GC_CITY_NAME}`-хаком) — владелец так и сделал. На живой машине: 8 процессов `deja mcp`
  (stdio, по паре на каждый из 4 процессов claude, 1241 MB RSS суммарно) против 2 процессов общего `kb` (329 MB).
- **MCP для pi gc не умеет** (нет проекции), хотя pi 1.5.0 поддерживает MCP нативно (`.pi/mcp.json`, stdio + HTTP).
- **«Нет конфига харнесс + агент» — для флагов CLI неверно, для всего остального верно.** `[providers.X]` с `base`-цепочкой,
  `option_defaults` (model/effort/permission_mode), `env`, `args_append` + переопределение на агенте работают и объяснимы
  (`gc config explain --provider`). Но hooks/MCP/skills/settings харнесса в `ProviderSpec` не входят: «общее для claude»
  выражается только про аргументы запуска.
- `[agent_defaults]` — полуфабрикат: из 11 полей рантайм применяет 5; `model`, `wake_mode`, `allow_*` «parsed but not applied»,
  `skills`/`mcp` — tombstone.
- `config.Agent`: **73 поля** (57 TOML + 14 runtime-only + 2 приватных). Новое поле = правка в **7 местах** + 5 сторожевых
  тестов. `internal/config`: 27 712 строк прод-кода, 57 852 строк тестов.
- 18 встроенных провайдеров; владелец использует 2 (19 ролей на claude, 2 на codex, pi объявлен, но не назначен ни одной роли).
  Полная глубина — только у claude и codex. Аудит «non-Claude provider parity» (issue #672) открыт с 2026-04-13.
- **Неожиданное для H3:** во встроенных пресетах `supports_acp=false` у codex и pi; у claude `true`, но без `acp_command`
  (а `claude --help` ACP-режима не показывает). В `src/pi-mono` нет ни одного упоминания ACP.
- Харнессы в 2026 сами умеют почти всё, что gc материализует: scopes настроек, плагины (skills+hooks+MCP), per-invocation
  флаги (`--settings`, `--mcp-config`, `--plugin-dir`; `-c`, `-p`; `-e`, `--skill`). Словарь hook-событий у Claude Code и Codex
  совпадает.
- AGENTS.md проекта объявляет «No skills system» и «No MCP/tool registration» *постоянными исключениями*, при этом
  `internal/materialize` (3374 строки) делает ровно это. Документация отстаёт от кода в трёх проверенных местах.

## 2. Факты

### 2.1 Встроенные провайдеры-харнессы

Определены в `internal/worker/builtin/profiles.go:240-246` (порядок) и `:247-1010` (спеки); `config.ProviderSpec`
(`internal/config/provider.go:65`, 32 поля) — пользовательская проекция того же. **[П]**

Параметризуется: `command`, `args`, `args_append`, `prompt_mode` (arg/flag/none) + `prompt_flag`, `ready_delay_ms`,
`ready_prompt_prefix` (скрейпинг приглашения TUI), `process_names`, `resume_flag`/`resume_style`/`resume_command`,
`session_id_flag`, `fork_flag`, `permission_modes`, `options_schema` + `option_defaults` (model/effort/permission/sandbox → флаги),
`env`, `instructions_file`, `print_args`, `title_model`, `supports_acp`, `acp_command`/`acp_args`, `supports_hooks`,
`upstream_env`, `program_status` (OSC 7501). **[П]** `provider.go:65-205`

| Провайдер | hooks | MCP-проекция | skills sink | sessionlog reader | OSC 7501 | ACP в пресете | canonical profile | тест-файлов¹ | глубина² |
|---|---|---|---|---|---|---|---|---|---|
| claude | да (city `--settings`) | да | да | да | да | `true`, без `acp_command` | да | 525 | 5 |
| codex | да | да | да | да | да | **нет** | да | 272 | 4 |
| gemini | да | да | да | да | — | нет | да | 111 | 3 |
| opencode | да (plugin) | да | да | да | — | да (`acp`) | да | 76 | 3 |
| **pi** | да (extension) | **нет** | да (`.agents/skills`) | да | да | **нет** | да | 47 | 3 |
| kimi | да | нет | нет | да | — | да | да | 49 | 2 |
| cursor | да | только stdio | нет | да | — | нет | да | 37 | 2 |
| mimocode | да | да | да | да | — | да | да | 27 | 2 |
| kiro | да, без pre-compact | нет | нет | да | — | да | нет | 31 | 2 |
| copilot | да | нет | нет | да | — | нет | нет | 29 | 2 |
| antigravity | да, без pre-compact | нет | нет | да | — | нет | да | 26 | 2 |
| omp | да | нет | нет | нет | да | нет | нет | 18 | 1 |
| grok | нет | нет | нет | да | да | нет | нет | 16 | 1 |
| zcode | нет | нет | нет | да | — | нет | да | 16 | 1 |
| amp | «not yet wired» | нет | нет | да | — | нет | нет | 10 | 1 |
| auggie | «not yet wired» | нет | нет | да | — | нет | нет | 9 | 1 |
| groq, cerebras | через opencode | [Н] | [Н] | через opencode | — | да | нет | 10 / 7 | 1 |

¹ Число `*_test.go` в `cmd`, `internal`, `test` с литералом `"<имя>"` — грубый прокси, измерено `grep -rIl`. **[П]**
² Моя шкала 1–5 по сумме столбцов. **[В]**

Источники столбцов **[П]**: hooks — `internal/hooks/hooks.go:34,183,287-297`; MCP — `cmd/gc/mcp_integration.go:84-91`,
`internal/materialize/mcp_project.go:390-399`; skills — `internal/materialize/skills.go:93-100`; readers — список файлов
`internal/sessionlog/*_reader.go`; OSC/ACP — `profiles.go:264,284,373,566,582,610,745,776,833,856,919,937`; profiles —
`profiles.go:1034-1058` (все десять — `<x>/tmux-cli`, ACP-профиля нет ни одного).

Специально для **codex**: подкоманда `resume`, `-c model_reasoning_effort=`, три permission-режима с алиасами устаревших
флагов (`profiles.go:339-445`); `.codex/hooks.json` с апгрейдером формата (~700 строк, `hooks.go:881-1600`, часть разбора общая с claude);
`internal/runtime/dialog.go` (2060 строк) нажимает клавиши в TUI на «Trust this folder?» и «Hooks need review»
(`dialog.go:1379,1434`) — при том что у codex 0.166 есть `--dangerously-bypass-hook-trust` (`codex --help`). **[П]**

Специально для **pi**: `args = ["-e", ".pi/extensions/gc-hooks.js"]` (`profiles.go:895`); расширение на 206 строк
(`internal/bootstrap/packs/core/overlay/per-provider/pi/.pi/extensions/gc-hooks.js`) — помимо хуков зеркалит транскрипт в
`GC_PI_TRANSCRIPT_DIR` и сообщает session id через env; `PI_PROGRAM_STATUS=1` (`profiles.go:919-921`). **[П]**
Пресет отстаёт от pi 1.5.0: `SessionIDFlag` не задан (`profiles.go:892-922`), хотя `pi --help` показывает `--session-id <id>`;
MCP-проекции нет, хотя есть `docs/mcp.md` и `packages/mcp` в pi-mono. **[П]**

Из 16 проверенных бинарников на машине установлены claude, codex, pi, opencode, agy; остальные 11 — нет
(`command -v`). Живьём «экзотику» проверить было нечем. **[П]**

### 2.2 Harness hooks

`internal/hooks` (1952 строки прод, 3646 тестов) — установщик *собственных* хуков gc. **[П]** `hooks.go:1-5`

- Claude: встроенный `internal/hooks/config/claude.json` сливается с пользовательским `<city>/.claude/settings.json` и пишется
  в `<city>/.gc/settings.json`, который передаётся как `--settings` (`hooks.go:660-674,768-822`, `cmd/gc/cmd_start.go:1441`).
  Файл **один на город** — для всех Claude-агентов. Для claude хуки считаются включёнными всегда (`resolve.go:751-763`).
- Остальные: файлы из `internal/bootstrap/packs/core/overlay/per-provider/<p>/` копируются в workdir агента
  (`hooks.go:323-360`); список — `install_agent_hooks` на workspace или агенте, replace, не merge (`resolve.go:149-160`).
- Что именно ставится (`internal/hooks/config/README.md`, таблица событий): session start → `gc prime --hook`;
  pre-compaction → `gc handoff --auto`; user prompt submit / before agent run → `gc hook run --timeout 15s -- nudge drain --inject`
  и `mail check --inject`. Других событий нет. **[П]**
- Команды `gc hooks install` **не существует** (нет в `gc --help`, нет `Use: "hooks…"` в `cmd/`), хотя на неё ссылаются
  заголовки `gc-hooks.js:3` и `gc-hook.ts:3`. `gc hook` — это про очередь работы (`gc hook --help`). **[П]**

Может ли автор пака объявить свои хуки один раз для claude+codex+pi? **Нет.** Доступно только: **[П]**
1. `<city>/.claude/settings.json` — только Claude, только весь город сразу.
2. `overlay/per-provider/<provider>/…` в паке или `overlay_dir` агента: файлы в формате *каждого* харнесса, JSON-merge
   для шести путей (`internal/overlay/merge.go:15-22`), пишутся в workdir (для rig-ролей — в корень репозитория рига).
3. `args_append` провайдера (так владелец отдаёт хуки Codex).

### 2.3 Точки расширения скриптами (хуки жизненного цикла gc)

**[П]** `docs/reference/config.md:93-133`, `config.go:1196-1199`, `cmd/gc/hooks.go:11-20`

| Точка | Когда |
|---|---|
| `pre_start` (агент) | до создания сессии |
| `session_setup`, `session_setup_script` | после создания сессии |
| `session_live` (агент; pack `[global]`) | на старте и при смене конфига без рестарта (tmux-тема и т.п.) |
| `on_boot`, `on_death` | старт контроллера / неожиданная смерть сессии |
| `scale_check`, `work_query`, `sling_query` | шаблоны shell-команд в реконсиляции и маршрутизации |
| `overlay_dir`, pack `overlay/` | копирование файлов в workdir на старте |
| orders (exec), pack `commands/`, pack `doctor/` | расписание / CLI-расширения / проверки |
| `[[service]]` (`workflow`, `proxy_process`), `[[webhook]]` | процессы под супервизором за `/svc/{name}`; входящие HTTP |
| `.beads/hooks/on_*` | gc-версии удалены, пользовательские не трогаются |

Это хуки *оркестратора*, не харнесса: ни один не срабатывает на события внутри хода агента.

### 2.4 MCP

- Источник: `mcp/<name>.toml` или `.template.toml` в паке и `agents/<name>/mcp/`; поля `name, description, command, args,
  env, url, headers` (`internal/materialize/mcp.go:86-94,106-115`). Слои по графу паков, одноимённые замещаются целиком. **[П]**
- Доставка: файл в workdir — `.mcp.json` (claude), `.codex/config.toml`, `.gemini/settings.json`, `opencode.json`,
  `mimocode.json`, `.cursor/mcp.json` (`mcp_project.go:49-83`); транспорт stdio + HTTP, SSE нет, cursor только stdio
  (`:390-399`). ACP-сессии получают серверы in-band в `session/new` (`cmd/gc/mcp_integration.go:174`). **[П]**
- `internal/runtime/mcp.go` (77 строк) — тип `MCPServerConfig` для ACP `session/new` и нормализация;
  `mcp_redact.go` (134) — вычищает секреты из env/headers/URL/args перед записью в метаданные сессии и ACP-capture. **[П]**
- Поля `mcp = [...]` и `skills = [...]` на агенте, patch, override и в `[agent_defaults]` — tombstone, игнорируются
  (`config.go:769-773,3612-3620`, `docs/reference/config.md:128,160,195`). Выборочно назначить сервер агенту можно только через
  каталог `agents/<name>/mcp/`. **[П]**
- Управления процессами MCP нет: в `internal/materialize` и `cmd/gc/*mcp*` только запись конфигов, отпечатки дрейфа и рестарт
  сессии при изменении. CLI — один `gc mcp list`. **[П]**
- В шаблонных данных MCP нет ни имени города, ни URL сервисов (`mcp_runtime.go:56-104`) — отсюда `${GC_CITY_NAME}` у владельца. **[П]**
- Гид `docs/guides/capabilities-for-coding-agent-users.md:66` всё ещё говорит «MCP is list-only today». **[П]**

### 2.5 Skills, slash-команды, prompts

- Skills: `skills/<name>/SKILL.md` в паке (всем агентам города) и `agents/<role>/skills/` (роли); symlink в sink:
  `.claude/skills`, `.agents/skills` (codex и pi), `.gemini/skills`, `.opencode/skills`, `.mimocode/skills`
  (`skills.go:93-100`). Allow-list нет: в scope каждый получает всё. Это единственная настоящая кросс-харнессная
  материализация в gc. **[П]** Тот же гид (строки 60-62) говорит, что pi пропускается, — устарело.
- Slash-команды и subagents харнессов gc не материализует; pack `commands/` — это подкоманды `gc <pack> …`. **[П]** `gc --help`
- CLAUDE.md/AGENTS.md gc не генерирует (кроме заглушек kiro `AGENTS.md` и `copilot-instructions.md` в core overlay);
  `instructions_file` — только имя для шаблонов. **[П]**
- Prompt: `prompt_template` (`.template.md`, Go `text/template`), данные `PromptContext` (21 поле, `cmd/gc/prompt.go:26-65`),
  функции `cmd`, `session`, `basename`, `templateFirst` (`prompt.go:441-460`), фрагменты `global_fragments` /
  `inject_fragments` / `append_fragments`. Доставка: аргумент/флаг запуска либо `gc prime --hook` из SessionStart-хука. **[П]**

### 2.6 Слои конфига

Порядок (по структурам и merge-функциям; точная последовательность применения — **[В]**):

1. builtin-пресет → 2. `[providers.X]` с `base`-цепочкой, `args_append`, `options_schema_merge`
(`provider.go:66-78`) → 3. `[[patches.providers]]` (`patch.go:246`) → 4. `[workspace]` (`provider`, `env`,
`install_agent_hooks`, `global_fragments`) → 5. `[agent_defaults]` пака и города → 6. агент (`agents/<n>/agent.toml`) →
7. `[[patches.agent]]` → 8. `[[rigs.overrides]]` / `[[rigs.patches]]` (`config.go:702`) → 9. `.gc/site.toml` → 10. env сессии.

Что работает **[П]**: `[providers.claude] option_defaults = {model, effort}` + `option_defaults` агента поверх
(`resolve.go:777-828`); `env` сливается аддитивно; provenance виден — вывод `command gc config explain --provider metropolis-codex`:
`chain: providers.metropolis-codex → providers.codex → builtin:codex`, `option_defaults.model = gpt-6.1-sol # providers.codex`.

Где ломается или не выражается **[П]**:
- `[agent_defaults]`: применяются только `provider`, `default_sling_formula`, `append_fragments`, `upstream`
  (`config.go:3571-3574,4338-4391`) и `context_advisory` (`cmd/gc/cmd_nudge.go:883`); `model` — «not yet auto-applied
  at runtime» (`config.go:3585-3588`).
- Провайдер пака с тем же именем, что у города, не сливается, а отбрасывается целиком (`pack.go:241,450,675,894`) — пак не
  может «добавить env городскому claude», только завести свой `X-claude` с `base`. Отсюда `metropolis-codex`,
  `servitors-codex`, `servitors-claude`.
- У `Rig` (22 поля) нет ни provider, ни env: «для этого рига другая модель» — только поагентными `[[rigs.overrides]]`.
- Настройки харнесса вне флагов (permissions, plugins, skillOverrides, hooks, MCP) не слоятся вообще: Claude — один файл на
  город; остальное — файлы в workdir, общие для всех ролей с этим workdir.

Цена поля: `Agent` (`config.go:3686`), `AgentPatch` (`patch.go:31`, 54 поля), `AgentOverride` (`config.go:702`, 53 поля),
`applyAgentMutation` (`patch.go:537`, 175 строк), `AgentOverride.toAgentPatch`, `Agent.Clone` (`config.go:4077`),
`deepCopyAgent` (`cmd/gc/pool.go:304`, 153 строки) + тесты `TestAgentFieldSync`, `TestApplyAgentPatchCoversAllFields`,
`TestApplyAgentOverrideCoversAllFields`, `TestAgentCloneIsDeep`, `TestDeepCopyAgentCoversAllFields`
(`internal/config/AGENTS.md`, раздел «Adding agent config fields»). **[П]**

### 2.7 Что владелец написал поверх gc

| Потребность | Костыль | Где |
|---|---|---|
| Декларативные хуки по ролям, claude+codex | `dispatcher.py` (722) + `hooks.d/*.toml` (168) + скрипты + `install.py` (179) = 4302 строки; роль определяется по `GC_TEMPLATE` внутри диспетчера, т.к. Claude-настройки одни на город | `gascity-packs/packs/metropolis/assets/hooks/`, README §9 (`:1900-1977`) |
| Хуки в Codex | 6 строк `-c hooks.<Event>=[…]` в `args_append` собственного провайдера | `packs/metropolis/pack.toml:28-37` |
| Путь к паку из хука | symlink `.gc/runtime/packs/metropolis/pack` + команда `gc metropolis hooks install` + doctor-проверка | README §9 `:1956-1961` |
| Общий MCP на город | `[[service]] kind="proxy_process"` → `serve.py` на unix-сокете → дочерний `bm mcp`; `mcp/kb.template.toml` с `url = "http://127.0.0.1:8372/v0/city/${GC_CITY_NAME}/svc/kb/mcp"`; `[workspace] env = {GC_CITY_NAME}` | `servitors/pack.toml:62-68`, `servitors/mcp/kb.template.toml`, `share/gc/city.toml:3` |
| MCP и хуки одного плагина (deja) в Codex | ещё 5 строк `-c mcp_servers.deja=…`, `-c hooks.*=…` | `servitors/pack.toml:36-49` |
| Пер-ролевые настройки Claude | `overlay_dir = "assets/claude-lean"` кладёт `.claude/settings.local.json` в workdir (для rig-ролей — в корень рига) | `packs/metropolis/agents/worker/agent.toml:5-7`, README §7 `:1797` |

Все строки таблицы **[П]**. Измерение процессов (`ps -eo pid,rss,args`, 2026-11-10): `deja mcp` — 8 процессов, 1241 MB
(один выброс 1.09 GB); процессов `claude` — 4; `kb` (`serve.py` + basic-memory) — 2 процесса, 329 MB. Оговорка: часть claude —
сессии этого исследования, а не города; закономерность «stdio-MCP на каждый процесс харнесса» от этого не меняется.

### 2.8 Что харнессы умеют сами

| | Claude Code 2.1.322 | Codex 0.166 | pi 1.5.0 |
|---|---|---|---|
| Слои настроек | managed > `--settings` > project local > project > user; `CLAUDE_CONFIG_DIR`, `--setting-sources` | packaged < MDM < system < user (+`-p` profile) < project `.codex/` < `-c` session flags (`codex-rs/config/src/config_layer_source.rs:7-37`); `CODEX_HOME` | user `<agent-dir>` (`PI_CODING_AGENT_DIR`) < project `.pi/` |
| Hooks | 33 события, типы `command`/`http`/`mcp_tool`/`prompt`/`agent`; в settings, плагине, frontmatter skill и subagent | 11 событий с теми же именами (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `PreCompact`, …) в `codex-rs/hooks/src` | TS-расширения, ~30 событий (`session_start`, `before_agent_start`, `tool_call`, `tool_result`, `session_before_compact`, …), `-e <path>` |
| MCP | `.mcp.json`, user scope, `--mcp-config`, `--strict-mcp-config` | `[mcp_servers]`, `codex mcp`, `-c mcp_servers.x=…` | `.pi/mcp.json`, `<agent-dir>/mcp.json`, stdio + HTTP, `--no-mcp` |
| Skills | `.claude/skills`, плагины | `.agents/skills` | `.pi/skills`, `.agents/skills`, `--skill <path>` |
| Пакет | plugin (`--plugin-dir`): skills, agents, hooks, MCP | plugin: skills, mcp_servers, apps, hooks (`codex-rs/plugin/src/manifest.rs:19-25`) | package: extensions, skills, prompts, themes |
| Subagents | `.claude/agents`, `--agents <json>` | `codex-rs/agent-roles` | [Н] |
| ACP нативно | нет в `claude --help` | 0 упоминаний в клоне | 0 упоминаний в клоне |

Источники **[П]**: `claude --help`, `codex --help`, `pi --help`; https://code.claude.com/docs/en/settings и `/hooks`
(прочитаны первые 100k символов страницы hooks); `pi-mono/packages/coding-agent/docs/{mcp,configuration,extensions,skills}.md`.
Про ACP: `grep -rIl -i 'agent client protocol|agentclientprotocol|--acp|"acp"'` по `*.md,*.ts,*.json` в pi-mono и
`agent client protocol|agentclientprotocol` по `*.md,*.rs,*.toml` в codex — пусто.

## 3. Вердикты

**H9a, «нет общего на город реестра MCP» — частично.** Реестр *определений* есть и слоится. Нет реестра *процессов*:
gc не запускает и не делит MCP-серверы, не связывает `[[service]]` с каталогом MCP, не даёт шаблону URL сервиса. Общий
HTTP-MCP возможен только ручной сборкой. Плюс дыра: pi не получает MCP совсем.

**H9b, «нет стандартизированных хуков» — подтверждена по сути, формулировка неточна.** Хуки харнессов gc ставит, но
это закрытый набор из четырёх событий под собственные нужды. Пользовательской декларативной кросс-харнессной модели нет;
4302 строки у владельца — прямое доказательство спроса.

**H9c, «нет слоёного конфига харнесс + агент» — частично.** Для аргументов запуска (model, effort, permission, env, args)
слои есть и работают лучше, чем утверждает владелец. Для hooks/MCP/skills/settings харнесса слоёв нет; `[agent_defaults]`
недоделан; пак не может дополнить городской провайдер; у рига нет уровня «provider/env».

**«Экзотические агенты только на бумаге» — в основном подтверждена, без живого прогона.** Не «только запись в таблице»:
у 15 из 18 есть собственный transcript reader, у 14 — hook-файлы, у 10 — canonical profile. Но полный набор (hooks + MCP + skills +
status + диалоги) есть только у claude и codex; amp/auggie сам код называет «not yet wired»; пресет pi отстаёт от pi 1.5.0;
parity-аудит #672 открыт 7 месяцев. Запустить экзотику было нечем (11 бинарников не установлены) — поведение в рантайме **[Н]**.

**H3 (частично моя).** В gc «из коробки» ACP не готов ни для одного из трёх критичных харнессов: codex и pi —
`supports_acp=false` (`gc config explain --provider codex|pi`), claude — `true` без `acp_command`, то есть ACP-запуск возьмёт
обычный `claude` (`provider.go:312-316`). Сами codex и pi ACP нативно не говорят (нужны адаптеры; для pi в клонах адаптера нет).
Работает ли claude-ветка на практике — **[Н]**, это тема отчётов по ACP.

**H7 (частично моя) — подтверждается на этом срезе.** Конфиг + hooks + overlay + materialize + dialog ≈ 36.8k строк прод-кода
(27 712 + 1952 + 1342 + 3374 + 2435) обслуживают 18 провайдеров, из которых владелец использует 2. `sessionlog` — ещё 17 011
строк на 15 форматов транскриптов.

## 4. Следствия для новой системы

Общие требования (из костылей владельца) **[В]**:
1. Три сущности: **harness profile** (харнесс + модель/effort/permission + env + набор capability-пакетов), **agent**
   (ссылка на profile + точечные переопределения), **rig override** (те же ключи, включая env и profile целиком).
2. Один универсальный merge (map-слияние по ключам с provenance) вместо параллельных структур `Agent/Patch/Override`.
   `gc config explain` с цепочкой происхождения — сохранить, это лучшее в текущем конфиге.
3. Доставка в харнесс — **флагами на запуск, а не файлами в репозитории рига**: `--settings`/`--mcp-config
   --strict-mcp-config`/`--plugin-dir` (claude), `-c`/`-p` (codex), `-e`/`--skill`/`--append-system-prompt` (pi). Это убирает
   управление `.gitignore`, adoption/refusal-логику и общий workdir как единицу настройки.
4. Реестр MCP с полями `scope` (city/rig/session) и `lifecycle` (managed/external): managed HTTP-сервер оркестратор запускает
   один раз и подставляет URL сам; stdio остаётся per-session. Делить можно только серверы без per-session состояния
   (браузер, например, нельзя).
5. Поддерживать ровно три харнесса; пресет — данные (файл), а не Go-код.

**Вариант A, тонкий: оркестратор не трогает конфиг харнесса.** Profile = имя + config-dir или набор флагов. Хуки, MCP,
skills пользователь пишет нативно (плагин Claude, плагин Codex, пакет pi). Оркестратору от харнесса нужны три вещи: начало
сессии (контекст), доставка сообщений, конец хода — при ACP это даёт сам протокол, хуки для этого не нужны.
Плюсы: почти нет кода материализации, нет дрейфа за версиями харнессов. Минусы: правило «запретить X всем ролям» пишется
трижды; смена config-dir уносит авторизацию (`auth.json` лежит в `<agent-dir>` pi — **[П]**; для claude/codex — **[Н]**),
поэтому профиль лучше собирать флагами поверх пользовательского каталога.

**Вариант B, толстый: оркестратор материализует сам.** Канонические `[mcp.*]`, `[[hook]]`, `skills/` → рендер под три
харнесса. Реалистично именно для трёх: словарь и JSON-протокол командных хуков у Claude и Codex совпадают (один диспетчер),
для pi нужен shim-расширение порядка 200 строк (как `gc-hooks.js`). Диспетчер владельца (722 строки) — готовый прототип.
Минусы: это тот же путь, что привёл gc к `hooks.go` с апгрейдерами форматов; каждое изменение формата харнесса — ваша работа.

**Рекомендую B только в трёх точках, остальное A** **[В]**: (1) реестр MCP с общими managed-серверами, (2) один
hook-диспетчер с декларациями `event/roles/harness/tools/run`, (3) каталог skills в `.agents/skills`-формате. Permissions,
плагины, subagents, slash-команды, темы — нативный конфиг харнесса, выбираемый профилем. Главный риск — пункт (2) под ACP:
доходят ли пользовательские хуки до сессии через адаптеры, не проверено.

## 5. Открытые вопросы и непроверенное

- Срабатывают ли хуки и читаются ли settings/плагины, когда харнесс запущен через ACP-адаптер (claude-agent-acp, codex-acp);
  какой адаптер брать для pi. Не проверял — вне моей темы, но от этого зависит выбор A/B.
- Работает ли в gc `session = "acp"` с builtin claude без `acp_command` — не запускал (запрет трогать город).
- Точный порядок применения слоёв 4–9 в п. 2.6 восстановлен по структурам, а не трассировкой загрузчика.
- Рантайм-поведение 13 «экзотических» провайдеров не проверялось: бинарников нет.
- Справочник владельца `/home/ubuntu/proj/share/gc/.claude/skills/gascity/references/config.md` по указанному пути
  отсутствует (каталога `gascity` в `.claude/skills` нет); использовал `city.toml`, `pack.toml` и README паков.
- Переносится ли авторизация claude/codex при смене `CLAUDE_CONFIG_DIR`/`CODEX_HOME` — не проверял.
- Страница hooks Claude Code прочитана на треть (100k из 291k символов); список событий и типов — из прочитанной части.
- Получают ли groq/cerebras MCP и skills через семейство opencode — не проследил.
- Выброс 1.09 GB RSS у одного `deja mcp` не разбирал.

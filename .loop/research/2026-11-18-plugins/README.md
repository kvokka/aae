# Исследование 2026-11-18: плагины aae и нативные субагенты

Сессия владельца с claude. Эксперименты шли во временных каталогах, скрипты не сохранены; здесь — выводы.
Сырые находки по codex и по вложенности — файлы рядом. Пакет WP-116 переносит проверенное в
`docs/design/experiments.md` с маркерами `MEASURE` и перепроверяет codex на текущей версии.

## claude 2.1.326, фейковый провайдер (находки агента, файл не сохранился)

Запуск как у aae: `-p --input-format stream-json --output-format stream-json --verbose --setting-sources project,local
--settings <f> --plugin-dir <плагин со скиллами> --append-system-prompt X --permission-mode bypassPermissions --agents '<json>'`.

1. `--agents` + `--append-system-prompt` без `--agent`: `init.agents` перечисляет агентов; список типов приходит
   главному потоку системным сообщением «Available agent types for the Agent tool: …»; системный промпт главного
   потока — дефолтный Claude Code + добавленный текст в конце.
2. Поля субагента в `-p` работают: `prompt` (системный промпт субагента = база SDK + его `prompt`;
   `--append-system-prompt` родителя в субагента НЕ попадает), `model`, `effort`, inline `mcpServers` (сервер стартует
   только для субагента; без `tools` его инструмент — за ToolSearch), `hooks` (срабатывают только на вызовах
   субагента; `Stop` приходит как `SubagentStop`), `skills` (предзагрузка: SKILL.md целиком в контекст; имена
   разрешаются из скиллов `--plugin-dir`, и `x`, и `aae:x`), `maxTurns`.
3. Хуки из `--settings` срабатывают внутри субагентов с `agent_id` и `agent_type`; у главного потока этих полей нет.
4. `permissions.deny: ["Agent(x)"]` убирает тип из списка и блокирует вызов, в том числе внутри субагентов;
   `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1` оставляет только определённых агентов (без него есть и встроенный
   `claude`). Субагент с инструментом `Agent` может звать любой определённый тип, себя тоже; списка типов он не
   получает — их называет его промпт.
5. Скилл на агента: `disallowedTools: ["Skill(x)"]` убирает весь инструмент Skill; точечно — только хук
   `PreToolUse` с матчером `Skill` в определении агента (deny по списку).
6. `--resume` без повторного `--agents` — «Agent type 'worker' not found»; с повтором работает.
7. `--agent <name>` заменяет системный промпт — aae его не использует (решение 143: роль добавляется к системному
   промпту). Агенты из `agents/` плагина (`--plugin-dir`) теряют `hooks` и `mcpServers`.
8. В главном потоке `Agent` по умолчанию асинхронный; во вложенных — синхронный.
9. По документации: с `--print` `--agents` принимает путь к JSON-файлу (2.1.281+) — снимает предел длины argv.

## Живые прогоны вложенности (haiku; codex `gpt-6-luna`)

- claude: из коробки глубина 2 (у второго уровня нет `Agent`); `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` = 1/2/3/5 задаёт
  её честно; `deny Agent(x)` и `disallowedTools: ["Agent"]` работают на вложенных уровнях; вызовы синхронные.
- codex 0.169: модель берёт multi-agent v2 (`collaboration`: `spawn_agent` + `wait_agent`); `agents.max_depth` в v2
  не действует; потолок — число тредов (`agents.max_threads`, в бинарнике есть
  `agents.max_concurrent_threads_per_session`); роли главного треда видны на уровнях 2 и 3; вызов синхронный. Ключ
  глубины v2, видимо, в отдельной таблице (рядом `default_subagent_model`) — не найден; владелец: лимит вложенности
  codex — на будущее.

## Компакция (только строки бинарников, не проверено)

Хуки `PreCompact` и `PostCompact` есть у claude 2.1.326 и codex 0.168. Порог: claude `autoCompactWindow`
(`modelSettings`, поле агента в `--agents`), `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`; codex `model_auto_compact_token_limit`.

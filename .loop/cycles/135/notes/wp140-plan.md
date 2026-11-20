# WP-140 · `context-budget` для codex — разведка и план

Автор: expert, цикл 135. Проверенный прототип правок хука и манифеста — `wp140-prototype.diff` рядом
(`git apply --check` проходит на HEAD `aa0cbcf`; прогонялся на поддельных rollout и входах, см. раздел 2.5).

Источники: исходники codex — `openai/codex`, ветка main, коммит `d5a478e` (2026-11-16), пути ниже от `codex-rs/`;
бинарь из mise — `codex-cli 0.170.0` (строки сверены `strings`); настоящий rollout 0.170.0 из тестового прогона
этого репозитория на поддельном провайдере; `docs/design/experiments.md` (эксперимент 9, codex 0.169.0).
Живые модели и демо не запускались. Что взято только из исходников и не прогонялось — помечено «(исх.)».

## 1. Факты

### 1.1 Откуда брать контекст

- Во входе каждого хука codex есть `transcript_path` — rollout потока (`sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl`).
  Перед каждым хуком codex дописывает файл на диск (`core/src/session/mod.rs:6070-6082`, `hook_transcript_path`
  → `ensure_rollout_materialized`). `null` бывает только у нелокального хранилища потоков — у app-server
  loop-manager оно локальное.
- Формат: JSONL, строка `{"timestamp","ordinal","type","payload"}`. Типы: `session_meta` (первая строка),
  `response_item` (`payload.type`: `message` с `role` и `content[].type == "output_text"`, `function_call` с `name`
  и строкой `arguments`, `custom_tool_call` с `name` и строкой `input` — так идёт `apply_patch`,
  `function_call_output`), `event_msg` (`payload.type`: `token_count`, `task_started`, `item_completed`,
  `task_complete`), `turn_context`, `token_usage_record`.
- Контекст — `event_msg`/`token_count`: `payload.info.last_token_usage.total_tokens` (вход вместе с кэшем + выход
  последнего запроса). Это то же число, от которого сам codex считает автосжатие
  (`core/src/context_manager/history.rs:1159-1177`, `protocol/src/protocol.rs:2766`).
  `payload.info.total_token_usage` — накопленный итог за поток, **не** контекст. `info` бывает `null` (событие
  только про лимиты, `protocol/src/protocol.rs:2670`) — такие строки пропускать.
- **Отставание на один запрос.** `token_count` запроса пишется после того, как отработали его инструменты
  (`core/src/session/turn.rs:3812-3826`; в настоящем rollout `token_count` стоит после `function_call_output`).
  В хуке инструмента последняя `token_count` — от предыдущего запроса; у первого вызова сессии её нет (0).
- `token_usage_record` (пишется сразу по ответу модели, `core/src/session/mod.rs:5723-5745`) убрал бы отставание,
  но это новый тип строки, в 0.167.x не подтверждён. Не берём: `hard` в дизайне ниже считает сам codex.

### 1.2 Что хуки codex принимают и что делают

Ответы разбираются строго: `deny_unknown_fields`, лишний ключ — хук `failed`, эффекта нет
(`hooks/src/schema.rs:88-100`, схемы `hooks/schema/generated/*.output.schema.json`).

| Событие | Вход | Что работает | Что нет |
|---|---|---|---|
| `SessionStart` | `session_id`, `transcript_path`, `cwd`, `model`, `permission_mode`, `source`; **без** `turn_id` | `hookSpecificOutput.additionalContext` → сообщение developer в историю (`core/src/hook_runtime.rs:890-906`) (исх.) | — |
| `SubagentStart` | то же + `turn_id`, `agent_id`, `agent_type` | `additionalContext` так же; срабатывает у порождённого потока (`hook_runtime.rs:134-160`) (исх.) | — |
| `PreToolUse` | + `turn_id`, `tool_name`, `tool_input`, `tool_use_id`; у дочернего `agent_id`, `agent_type` | `permissionDecision: "deny"` с причиной: вызов не выполняется, модель получает «blocked by PreToolUse hook: …» (`hook_runtime.rs:240-262`) | `continue:false`, `stopReason`: хук `failed`, **команда выполняется** (`hooks/src/engine/output_parser.rs:369-379`; эксперимент 9, `experiments.md:503`) |
| `PostToolUse` | + `tool_response` | `additionalContext` → сообщение developer, модель видит его в следующем запросе (`core/src/tools/registry.rs:1046-1055`) (исх.) | `continue:false` ход **не** завершает: результат инструмента заменяется текстом `stopReason` (`hooks/src/events/post_tool_use.rs:216-232`, `registry.rs:1079-1097`) (исх.). Срабатывает только после успешного вызова (`registry.rs:1025-1031`) |
| `PreCompact` | `session_id`, `turn_id`, `transcript_path`, `cwd`, `model`, `trigger`; у дочернего `agent_id`, `agent_type` | `continue:false`: сжатия нет, ход кончается `interrupted`, каждый следующий ход над лимитом — так же; у дочернего кончается только его ход (`experiments.md:500`) | `decision: "block"` — неизвестный ключ, хук `failed`, сжатие идёт; exit 2 — то же (`experiments.md:502`). Ответ знает только `continue`, `stopReason`, `suppressOutput`, `systemMessage` |

Имя инструмента субагента во входе — `spawn_agent` (v1 и пространство по умолчанию, `core/src/tools/registry.rs:1212-1220`);
в multi-agent v2 имя — пространство имён, слитое с именем (`flat_tool_name`, `core/src/tools/mod.rs:53-67`), то есть
оканчивается на `spawn_agent` (исх., живьём не проверено).

### 1.3 Автосжатие

- Порог — `model_auto_compact_token_limit`; `native.codex` уходит флагами `-c` процессу
  (`crates/loop-manager-codex/src/part.rs:33-42`), значит действует на все потоки процесса и входит в отпечаток.
- **Отключить нельзя и выше 90% окна модели поднять нельзя:** действующий порог —
  `min(заданный, context_window * 9 / 10)` (`protocol/src/openai_models.rs:583-594`) (исх.; эксперимент 9 с
  «900000 — сжатия нет» этому не противоречит: там ответ был 150000 при окне 272000). Окно моделей каталога —
  272000 (порог ≤ 244800), у части `max_context_window` 872000; `model_context_window` поднимает окно не выше
  `max_context_window` (`models-manager/src/model_info.rs:19-28`).
- Сжимает посреди хода, после вызова, чей ответ пересёк порог (`experiments.md:497`).
- «Запретить сжатие и идти дальше», как `decision: block` у claude, в codex нечем: единственный ответ, который не
  даёт сжать, — `continue:false` из `PreCompact`, и он же завершает ход.

### 1.4 Чем останавливать сессию

Хук инструмента остановить ход не может (1.2). Останавливает только `PreCompact` + `continue:false`. Отсюда решение:
порог автосжатия ставится **равным `hard`**, и попытка сжатия становится событием «`hard` достигнут», которое
поднимает сам codex по своему точному счёту. Ход кончается `interrupted` → адаптер отдаёт `cancelled`
(`crates/loop-manager-codex/src/stream.rs:233`) → ядро, если исход уже объявлен, переводит прогон в `finishing`
при любом `stop_reason` (`crates/loop-manager-core/src/run/events.rs:233-257`). Хук объявляет исход
(`loop-manager finish <cut>`) до ответа, так что прогон заканчивается исходом `cut`.

### 1.5 Дочерние потоки

- Дочерний поток наследует хуки и порог родителя; файл роли порог не меняет (`experiments.md:499`, v1; v2 не
  проверен). `core/src/agent/child_config.rs:147-153` сбрасывает порог только под фичей
  `subagent_default_context_limits` (в разработке, по умолчанию выключена).
- Во входе хука дочернего потока: `session_id` — сессия корня (общая для всех потомков,
  `core/src/session/session.rs:669-672`), `agent_id` — id дочернего потока, `agent_type` — роль
  (`hook_runtime.rs:1099-1106`), `transcript_path` — **собственный** rollout дочернего потока
  (`hook_runtime.rs:206-218`: путь берётся у сессии, в которой идёт вызов) (исх.). Искать файл, как у claude, не надо.

### 1.6 Окружение хука

Хук codex идёт с окружением процесса; адаптер дописывает в команду `export LOOP_MANAGER_RUN=… LOOP_MANAGER_ATTEMPT_TOKEN=…
LOOP_MANAGER_BEAD=…` и актора трекера (`part.rs:187-211`), `BEADS_DIR` приходит слоем проекта. `br comments add` и
`loop-manager finish` из хука работают так же, как у claude; своих переменных codex хуку не ставит.

## 2. Дизайн

### 2.1 `addons/context-budget/addon.toml`

- `harnesses = ["claude", "codex"]`, `version = "0.3.0"`.
- В `[agent]` одна строка: `native.codex.model_auto_compact_token_limit = "${params.hard}"` с комментарием
  «codex compacts at `hard`; its `PreCompact` hook stops the session there instead». Ссылка-одиночка берёт тип
  параметра: `config explain` агента codex даёт `native.codex = {"model_auto_compact_token_limit":240000}  # addon
  context-budget` (проверено на копии манифеста, `hard = 240000`); агент claude получает только `native.claude`.
- Хуки — те же пять, без изменений: `SubagentStart` и `PreCompact` codex знает, пустой `matcher` — «все».
- Кода адаптера и ядра пакет не трогает.

### 2.2 `addons/context-budget/hooks/budget` (169 → ~185 строк)

1. **Распознать codex:** `codex=$(field .turn_id)`. `turn_id` обязателен во входе всех событий codex, где поведение
   расходится (`PreToolUse`, `PostToolUse`, `PreCompact`, `SubagentStart`), и отсутствует у claude. У `SessionStart`
   его нет, но там ответ одинаков. Отвергнуто: признак по rollout (имя файла или первая строка `session_meta`) —
   тогда остановка зависела бы от формата транскрипта, а с `turn_id` `PreCompact` останавливает сессию, даже если
   транскрипт не прочитался.
2. **`transcript()`:** для codex — `transcript_path` как есть, и у дочернего потока тоже
   (`[ -z "$agent" ] || [ -n "$codex" ]`).
3. **`tokens()`:** в предфильтр `grep` добавить `"token_count"`, в jq — ветку
   `elif .type == "event_msg" and .payload.type == "token_count" then .payload.info.last_token_usage.total_tokens // empty`.
   Фильтр один на оба формата: строки чужого формата дают `empty`.
4. **`dump()`:** два jq-фильтра получают ветку codex: вызовы — `response_item` с `payload.type` `function_call` или
   `custom_tool_call` → `"\(.name): \(.arguments // (.input | tojson) | .[0:240])"`; тексты — `message` с
   `role == "assistant"`, `content[]` типа `output_text`.
5. **`deny()`:** второй аргумент «завершить ход» (по умолчанию нет). `continue:false`/`stopReason` добавляются только
   при остановке claude; для codex — никогда (иначе хук `failed` и команда выполнится). Вызовы после `hard`:
   `deny "$stopped" "$ends"`, где `ends=true`, для codex `false`.
6. **`PreCompact`:** для codex — `cut_off "$main"` (дамп, `finish <cut>` один раз, ответ
   `{"continue":false,"stopReason":…}` — ровно то, что `cut_off` уже печатает для не-`PreToolUse`); для claude —
   прежний `decision: block`.
7. **Субагент:** `Agent | Task | *spawn_agent`.
8. Проверка `hard` по транскрипту в хуках инструментов остаётся общей: для codex она вторая линия (отказ в
   инструментах после остановки, дамп тем, кто увидел раньше); `cut_off` идемпотентен через `.stop`.
   `PostToolUse` за `hard` отвечает как у claude — в codex это заменит результат инструмента текстом остановки.
9. Шапка файла: «The hook … for claude and codex».

### 2.3 Находка по WP-139 в той же функции (claude)

Сейчас `deny()` всегда несёт `continue:false`, в том числе при отказе в субагенте за `soft`
(`hooks/budget:85-89`, вызов на строке 145). У claude `continue:false` из `PreToolUse` завершает ход
(`experiments.md:488`): агент, вызвавший субагента за `soft`, останавливается без исхода, хотя README и решение 149
говорят только «a call is denied» и дальше «wrap up». Правка п. 5 это чинит (отказ за `soft` — без остановки на обоих
харнессах). В claude-тест добавить одну проверку: у отказа за `soft` нет ключа `continue`. Запись в журнал — раздел 5.

### 2.4 Тесты

`crates/loop-manager/tests/e2e/context_budget.rs` — 392 строки из 400 (`scripts/budget.sh:39-40`). Тесты codex — в
новом `crates/loop-manager/tests/e2e/context_budget/codex.rs`, подключается строкой `mod codex;` в конце
`context_budget.rs`; дочерний модуль видит приватные `Bed`, `explain`, `note`, `commit`, `addon` (`use super::*`).
В самом `context_budget.rs` — только `mod codex;`, правка шапки и одна проверка из 2.3.

Помощники в `codex.rs`: `rollout(bed, name, tokens) -> String` (пишет поддельный rollout) и
`codex(bed, json) -> Value` (добавляет `turn_id`, `model`, `transcript_path`; `Bed::hook` свой путь не подставит,
если `transcript_path` уже задан). Бюджет стенда прежний: 1000 / 7000 / шаг 500.

Поддельный rollout (форма 0.170.0, по строке на объект):

```json
{"timestamp":"2026-11-19T04:53:02.707Z","type":"session_meta","payload":{"id":"t1","cli_version":"0.170.0","source":"vscode"}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"the usage of the parser"}]}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"I read the parser first."}]}}
{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo test -p parser\"}","call_id":"c1"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"a \"token_count\" in a tool output"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":100},"last_token_usage":{"input_tokens":80,"cached_input_tokens":40,"output_tokens":20,"total_tokens":100},"model_context_window":258400}}}
{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/parser.rs\n","call_id":"c2"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":99999},"last_token_usage":{"total_tokens":<tokens>},"model_context_window":258400}}}
{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex"}}}
```

Ловушки: накопленный `total_token_usage` 99999, хвостовая `token_count` с `info: null`, `"token_count"` в выводе
инструмента.

Случаи (те же, что у claude):

1. **Манифест.** `config explain` агента с `harness = "codex"` и `hard = 240000`: строка `native.codex =
   {"model_auto_compact_token_limit":240000}  # addon context-budget`, аргументы хука, пять событий; `native.claude`
   у него нет.
2. **Ниже `soft` тихо; `soft`.** `SessionStart` — заметка о бюджете. 900: `PostToolUse` и `PreToolUse spawn_agent` —
   пусто. 1200: заметка «Your context is 1k tokens, past your budget of 1k.» с `call no subagents`,
   `br comments add "$LOOP_MANAGER_BEAD"`, `loop-manager finish`; `PreToolUse spawn_agent` — `deny` **без** ключей
   `continue` и `stopReason`; `Bash` проходит. 1600 — тихо, 1700 — заметка снова. `finished` пуст,
   `kept_hard_quiet`.
3. **`hard`.** По циклу `(cut, событие, tokens)`: `("fail", "PreCompact", 6900)` — решает счёт codex, а не замер
   хука; `("pass", "PostToolUse", 7100)`; `("fail", "PreToolUse", 7100)`. Проверки: `finish <cut>` один раз; на биде
   один комментарий с `# context-budget: the session was stopped`, `## git status --short`, `parser half`,
   `?? notes.md`, `exec_command: {"cmd":"cargo test -p parser"}`, `apply_patch: `, `I read the parser first.`;
   ответ `PreCompact` и `PostToolUse` — ровно `{"continue":false,"stopReason":<reason>}` (сравнение целиком:
   лишний ключ codex отвергнет); ответ `PreToolUse` — `deny` с причиной и без `continue`. Дальше при малом
   транскрипте `PreToolUse` любого инструмента — `deny`; повторный `PreCompact` не объявляет исход второй раз.
4. **Дочерний поток.** Вход с `agent_id: "t2"`, `agent_type: "helper"`, `transcript_path` — rollout дочернего
   (1200), у корня свой rollout (6900). `SubagentStart` — «return your summary»; `PostToolUse` — заметка с «return
   your summary to your caller», без `loop-manager finish`. `PreCompact` дочернего: `continue:false`, `finished`
   пуст, комментарий «The subagent t2 of the session s1»; его `PreToolUse` — `deny`. Корень идёт дальше:
   `PostToolUse` — заметка «6k», `PreToolUse Bash` — пусто.
5. **`PreCompact` claude** остаётся `decision: block` (уже есть в claude-тесте; вход без `turn_id`).

Тест `demo_4_gives_its_claude_agent_a_small_budget` не меняется: `demos/` не трогаем, агент codex демо аддон не
подключает.

### 2.5 Что проверено прототипом

Копия аддона во временном каталоге с `wp140-prototype.diff`, поддельные rollout и входы: все случаи 2–4 из 2.4 дали
ожидаемые ответы, вход claude на `PreCompact` — прежний `decision: block`, в дампе `exec_command: {"cmd": …}`,
`apply_patch: "*** Begin Patch\n…"` и текст агента. Rust-тесты не писались и не гонялись.

### 2.6 Документы (в той же правке, по-английски)

- `addons/context-budget/README.md`: вводная (claude и codex, убрать «There is no codex side yet»); раздел
  «On codex» — список из раздела 3; в «Params» — строка о том, что на codex `soft` и `hard` задаются под окно модели.
- `docs/guide/addon-development.md:164-216`: выдержка манифеста (версия, `harnesses`, строка `native.codex`) и
  пункты «The measure», «Stopping», «No compaction» — по харнессам; `:232-234` — про тесты codex.
  `crates/loop-manager/tests/e2e/guides.rs` грузит примеры гайдов — прогнать его.
- `docs/architecture.md:60`: «for claude sessions» → «for claude and codex sessions».
- `docs/design/config.md:57-62` (диалект хуков codex): добавить — ответ `PreToolUse` с `continue: false` или
  `stopReason` проваливает хук, и вызов идёт; неизвестный ключ в ответе проваливает хук; `PreCompact` не знает
  `decision`.
- `docs/design/experiments.md:569` («codex: … any size»): уточнить — не выше 90% окна модели (по исходникам, без
  маркера MEASURE: это не замер).

## 3. «codex не даёт» — для README (раздел «On codex»)

1. **Нет запрета сжатия без остановки.** `PreCompact` не знает `decision: block`. Аддон ставит порог сжатия codex
   равным `hard` и отвечает на попытку `continue: false`: сжатия нет, ход кончается, сессия остановлена. Попытка
   сжатия и есть `hard`.
2. **Бюджет ограничен окном модели.** Порог сжатия не поднимается выше 90% окна (272000 у моделей каталога →
   244800). `hard` выше этого codex не дождётся: остановит на 90% окна, с тем же дампом и исходом. `soft` выше —
   заметки не будет вовсе. Значения по умолчанию (300000/420000) под окно 272000 не подходят: агенту codex задавать
   свои, например `params = { soft = 180000, hard = 240000 }`, либо поднимать окно через
   `native.codex.model_context_window`, где модель это позволяет.
3. **Хук инструмента ход не завершает.** `PreToolUse` принимает только отказ в одном вызове; `continue: false` там
   — ошибка хука, и команда выполняется. За `hard` каждый вызов получает отказ, а ход завершает `PreCompact`.
4. **Замер отстаёт на один запрос модели.** Использование запроса попадает в транскрипт после его инструментов:
   заметка за `soft` приходит на шаг позже, чем у claude. На `hard` не влияет — его считает codex.
5. **Заметка — только после успешного вызова.** `PostToolUse` не срабатывает на вызове, который упал.
6. **Субагент** — дочерний поток с порогом и хуками родителя; его инструмент — `spawn_agent`. Проверено на
   multi-agent v1; v2 — по исходникам.
7. **Конец хода — `interrupted`.** loop-manager видит `cancelled`; исход к этому моменту объявлен хуком. Если
   финишер исход не принял, ход всё равно остановлен, и прогон получает ход без исхода.

## 4. Объём и порядок

Размер S–M, одна сессия кодера (~60–80k контекста).

| Файл | Правка |
|---|---|
| `addons/context-budget/addon.toml` | +2 строки, версия |
| `addons/context-budget/hooks/budget` | +16 строк (прототип готов) |
| `crates/loop-manager/tests/e2e/context_budget/codex.rs` | новый, ~160 строк |
| `crates/loop-manager/tests/e2e/context_budget.rs` | +3 строки (до ~395 из 400) |
| README аддона, `addon-development.md`, `architecture.md`, `config.md`, `experiments.md` | ~40 строк |

Порядок: (1) `git apply` прототипа, сверить глазами; (2) `codex.rs` и проверка в claude-тесте; (3)
`cargo nextest run -p loop-manager -E 'test(/context_budget|guides/)'`; (4) документы; (5) записи в журнал;
(6) коммит, `scripts/loop/checks 135`.

Готово, когда: случаи 1–5 из 2.4 зелёные; README называет пункты раздела 3; документы из 2.6 совпадают с кодом;
`mise run ci` зелёный. Acceptance, демо и живой codex в условие не входят.

Не делать: `token_usage_record`; автоподбор бюджета под окно модели; `model_context_window` в манифесте; правки
адаптера codex; аддон у агента codex демо 4; стенд на поддельном Responses API (пакет его не просит).

## 5. Записи в журнал (кодеру — при сдаче)

- **РЕШЕНИЕ.** План пакета предполагал порог автосжатия codex «выше `hard`». Сделано иначе: порог равен `hard`.
  Причина: порог не поднимается выше 90% окна модели и сжатие нельзя запретить, не завершив ход; зато ответ
  `continue:false` на попытку сжатия — единственный способ остановить сессию codex из хука. Попытка сжатия стала
  событием «`hard` достигнут».
- **НАХОДКА.** Чего codex не даёт — семь пунктов раздела 3 (коротко, со ссылкой на README аддона).
- **ВОПРОС.** Значения по умолчанию `soft = 300000`, `hard = 420000` больше окна моделей codex (272000): агент
  codex с настройками по умолчанию не получит заметку и будет остановлен примерно на 245k. Варианты: (а) оставить
  общие значения и требовать в README задавать параметры агентам codex — принято до ответа; (б) аддон сам поднимает
  окно (`native.codex.model_context_window`) — это меняет размер запросов и, возможно, их цену; (в) отдельные
  значения по умолчанию на харнесс — в манифесте аддона такого механизма нет.
- **НАХОДКА.** WP-139, claude: отказ в субагенте за `soft` нёс `continue:false` и завершал ход без исхода
  (раздел 2.3); исправлено вместе с правкой `deny()`.
- Не проверено живьём (только исходники): `additionalContext` из `PostToolUse` и `SubagentStart` доходит до модели;
  `continue:false` из `PostToolUse` заменяет результат инструмента; имя инструмента субагента в multi-agent v2;
  собственный `transcript_path` у дочернего потока; потолок порога в 90% окна.

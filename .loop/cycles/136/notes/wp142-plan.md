# WP-142 · Пак `devloop`: цикл implement/review по эпику — план реализации

Автор — expert, цикл 136. Эскизы, проверенные на собранном `target/debug/loop-manager` и на `br` 0.7.6, лежат рядом:
`.loop/cycles/136/notes/wp142-sketch/` (`config.toml`, `loop-manager.toml`, `epic.toml`, `policy.yaml`, два шаблона
промптов, `gate.sh`, `tests.sh`, `selftest.sh`). Проект, собранный из них во временном каталоге, проходит
`loop-manager config check` (exit 0, пустой вывод), `selftest.sh` печатает `devloop selftest: ok`, пять мутаций
`gate.sh` и одна `tests.sh` им убиты. Кодер переносит эскизы в `devloop/` и правит по месту; живой запуск пака не нужен.

## 0. Что выяснено (факты, на которых стоят решения)

**br 0.7.6.**
- Политика лежит в `.beads/policy.yaml`, а не в `.beads/config.yaml` (в пакете плана имя файла названо неточно).
  Ключи, проверенные опытом: `workflow.strict`, `workflow.statuses`, `workflow.transitions` (с `initial`),
  `workflow.status_groups.ready`, `workflow.required_fields` (`"in_progress -> rework": [transition_comment]`).
- Встроенные статусы: `open, in_progress, blocked, deferred, draft, closed, tombstone, pinned`. Без политики `br update
  -s <любое слово>` проходит (даже `foo`). С `strict: true` и списком `statuses` чужой статус — отказ (exit 4).
  `rework` — имя самого br для «вернули с ревью» (его справка: `status_groups.ready: [open, rework]`).
- `br update -s closed` запрещён (только `br close`). `--transition-comment` пишет комментарий атомарно с переходом;
  на обновление без смены статуса (`rework -> rework`) — отказ `VALIDATION_FAILED`.
- `br update <id> -s rework` не трогает исполнителя (assignee). Повтор `-s rework` на `rework` без комментария — exit 0.
- Эпик без детей попадает в `br ready` (значит, `bead.ready` на него приходит).
- `BR_OUTPUT_FORMAT=toon` переводит вывод `br show` / `br comments` в TOON; `--json` его перекрывает.
- `br comments add <id> -f <файл>` — комментарий из файла; `br label add|remove <id> needs-owner`.

**loop-manager.**
- Адаптер `br` читает статус как текст и ничего по нему не решает (`crates/loop-manager-br/src/parse.rs:30`); захват —
  `update --claim` (ставит `in_progress`), отпускание — `update -s open --assignee ""`, закрытие сверяет только
  исполнителя (`lib.rs:171-213`). Опыт на временной базе со строгой политикой: захват → `rework` → отпускание и
  захват → `rework` → `br close` проходят.
- Исход exec-шага: `data = {exit_code, stdout, stderr}`, **последние 4 KiB** каждого (contracts 6.2). Упавший
  exec-шаг без `back_to` валит прогон — значит, `tests` обязан выходить с 0 всегда.
- `inherit_env` есть на шаге (`config.md`, «Formula»); `explain` показывает `exec.inherit_env = true  # formula epic`.
- В шаблоне нельзя назвать шаг, стоящий позже: кодер не может взять `steps.tests.*`; итог тестов до него доходит
  через `{{ remarks }}` (вывод `gate`) и комментарий ревьюера.
- В `RunView` нет номера раунда: в промпт его не подставить.
- Провал прогона отпускает бид → `bead.ready` → триггер стартует снова, пока не наберётся `bead_failed_runs`.
- Сессия claude пака читает `.claude/settings.json` репозитория в любом режиме (при наследовании хоста источники
  настроек не сужаются, без него — `--setting-sources project,local`, `part.rs:243`): хуки старого лупа в ней
  работают. Без `LOOP_MANAGER_LOOP=1` от них остаётся запрет нескольких shell-команд и слежение за контекстом
  субагентов (`loop_guard.py:266-348`).

## 1. Решения по открытым вопросам

**1.1. Запуск — и триггер по метке, и `run start` руками.** Триггер `epic-ready`: `on = "bead.ready"`,
`where = { "bead.labels" = "auto" }`, формула `epic`, `rig = "repo"`. Метка остаётся `auto`: в `Bead` нет типа бида,
отфильтровать «только эпики» нечем, а метка отделяет эпик-задачу от бида без метки. Руками:
`loop-manager run start devloop repo epic --input epic=<id>`. Отрицания в `where` нет, поэтому `needs-owner` запуск не
удерживает — удерживает гейт бида (1.6).

**1.2. Статусы эпика — четыре, пишут их демон и `gate.sh`, агенты статус не трогают.**

| Статус | Что значит | Кто ставит |
|---|---|---|
| `open` | заведён и ждёт прогона, либо прошлый прогон упал | владелец (`br create`); демон при `failed`/`cancelled` |
| `in_progress` | прогон идёт, ревью ещё ни разу не возвращало | демон (захват бида) |
| `rework` | ревью вернуло работу, идут следующие раунды | `gate.sh`, когда гейт не пройден |
| `closed` | ревью довольно, прогон `passed` | демон |

Обоснование: решение о возврате принимает `gate.sh` (оценка < 9, не `complete`, вердикта нет), ему и ставить статус;
скрипт не забывает, агент может. Обратно в `in_progress` никто не переводит: `rework` держится до конца прогона —
это и есть «на доработке после ревью». `--transition-comment` не используется: со второго возврата статус уже
`rework`, а на обновление без смены статуса br его не принимает; причина возврата и так лежит в комментарии ревьюера.
В `devloop/.beads/policy.yaml` — только `strict` и список статусов (эскиз `policy.yaml`): br тогда отказывает
выдуманному статусу. Граф переходов и `required_fields` не заводим: лишний способ сломать захват и закрытие демона.

**1.3. Аддоны — локальным путём.** Тег один, `v0.0.1`; в нём `addons/context-budget` версии 0.1.0 (сжатия, `window`,
`max_compactions`), `addons/dcg` нет. Тега с новым аддоном нет → `[[addon]] path = "../addons/context-budget"` и
`path = "../addons/dcg"`. README говорит, что сменить после релиза: `git = "<адрес репозитория>"`, `rev = "<тег>"`,
`dir = "addons/context-budget"` (и то же для `dcg`). Второй довод за путь: `config check` входит в `mise run ci`, а с
`git` он на холодном кеше ходит в сеть.

**1.4. Окружение.** Сессии (`[settings]` в `loop-manager.toml`, действует и на субагентов):
- `env.RUSTUP_TOOLCHAIN`, `env.RUSTUP_HOME`, `env.CARGO_HOME` — ссылками `${…}`. `cargo` в `PATH` — прокси rustup; без
  `RUSTUP_TOOLCHAIN` он берёт тулчейн хоста по умолчанию. `mise env` в репозитории выставляет все три (проверено),
  так что у демона, запущенного из окружения mise, они есть; без них сессия падает с `config_error` и именем
  переменной — понятная ошибка вместо тихой сборки не тем тулчейном.
- `env.BR_OUTPUT_FORMAT = "toon"` — решение из журнала (цикл 133, запись 13:30).
- Три переменные `CLAUDE_CODE_*` остаются как есть.
- Больше ничего: `mise`, `br`, `dcg`, `jq`, `cargo-nextest` находятся по `PATH` демона, конфиги — по `HOME`;
  `BEADS_DIR`, `BD_ACTOR`, `LOOP_MANAGER_*` даёт loop-manager.

`inherit_env = true` — на шагах `tests` и `gate`, из `[exec]` проекта убрать. `tests`: `mise run ci` должен видеть то
же окружение, что у владельца. `gate`: `git push` аутентифицируется агентом ssh или токеном оболочки.

**1.5. Кеш промптов — 5 минут.** claude 2.1.328 (строки бинаря): ключи настроек `promptCacheTtl` и
`subagentPromptCacheTtl` (`"5m"` | `"1h"`); по умолчанию главный разговор на подписке — 1 час, субагенты — 5 минут.
Порядок: `FORCE_PROMPT_CACHING_5M` → `CLAUDE_CODE_PROMPT_CACHE_TTL` / `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL` →
ключи настроек → `ENABLE_PROMPT_CACHING_1H` → автоматика. В пак — ключи настроек (у них есть описание в схеме claude,
это не обходной флаг): `native.claude.promptCacheTtl = "5m"`, `native.claude.subagentPromptCacheTtl = "5m"`; `explain`
показывает их рядом с `autoCompactWindow` аддона. codex 0.170.0: в запросе есть `prompt_cache_options.ttl`, но ключа
конфига или переменной для него в бинаре не нашлось — в журнал; пак работает только на claude. Проверить TTL без
живой сессии нельзя (в транскрипте `usage.cache_creation.ephemeral_1h_input_tokens` должен быть 0) — тоже в журнал.

**1.6. Прочее, что решено по ходу.**
- Формула — `epic` (`formulas/epic.toml`), вход — `epic` типа `task`; `formulas/task.toml` — `git mv`.
- `bead_failed_runs = 1` в `devloop/config.toml`: упавший прогон закрывает гейт `bead:devloop/<id>` и ждёт владельца
  («кончились циклы — ждём человека»). С порогом 3 триггер сам запустил бы ещё два прогона по 50 раундов.
- Вердикт гейт берёт из `finish --data` ревьюера (`LOOP_MANAGER_STEP_REVIEW_DATA`), а не из комментария: это исход
  шага именно этого раунда, старый комментарий за свежий принять нельзя. Комментарий — для людей и следующего кодера.
- `last_comments = 8` на обоих ai-шагах: четыре раунда (итог кодера + ревью). Без ограничения на 40-м раунде в промпт
  легло бы 80+ комментариев. Отсюда требование к ревью: комментарий самодостаточен (4.2).
- `review`: `max_attempts = 2`, у аддона `cut` по умолчанию (`fail`). Остановленный или сорвавшийся ревьюер
  повторяется один раз новой сессией (как `loopctl` повторяет ревьюера), дальше прогон падает и ждёт владельца.
  У `code` — `cut = "pass"` и `max_attempts` по умолчанию: `finish fail` кодера — это «заблокирован», повтор не нужен.
- Комментарии эпика — по-русски (их читает владелец, они заменяют `.loop/`), первые строки-маркеры — по-английски.
- Длинные результаты субагентов и лог тестов — в `target/devloop/` (в `.gitignore`, в git не идут). Что должно
  пережить раунд — в комментарий эпика.
- Коммиты эпика начинаются с `<id эпика>: ` — по ним ревьюер видит историю эпика целиком.

## 2. Формула `devloop/formulas/epic.toml`

Полный текст с комментариями — `wp142-sketch/epic.toml`. Суть:

```toml
name = "epic"
rounds = 50
[inputs]
epic = { type = "task", required = true }

[[step]]
name = "code"
kind = "ai"
agent = "coder"
last_comments = 8

[[step]]
name = "tests"
kind = "exec"
command = ["sh", "devloop/scripts/tests.sh"]
timeout = "60m"
inherit_env = true

[[step]]
name = "review"
kind = "ai"
agent = "reviewer"
max_attempts = 2
last_comments = 8

[[step]]
name = "gate"
kind = "exec"
command = ["sh", "devloop/scripts/gate.sh"]
timeout = "10m"
inherit_env = true
back_to = "code"
```

- `tests` — не чек (`back_to` нет) и не падает: `tests.sh` всегда выходит с 0. Ревьюер получает его вывод именем
  `{{ steps.tests.data.stdout }}` (последние 4 KiB), поэтому скрипт печатает выжимку в конце: при красном — хвост лога
  (25 строк по 120 символов), затем `HEAD:`, `UNCOMMITTED: <n> paths`, `TIME:`, `LOG: target/devloop/ci.log`,
  `RESULT: pass | FAIL (mise run ci: exit <n>)`. Целый лог ревьюер читает по пути.
- `gate` — единственный чек; его вывод уходит кодеру следующего раунда как `{{ remarks }}`.
- Таймаут ai-шагов — по умолчанию (4 ч): циклы нынешнего лупа идут от 10 минут до 2 часов.

## 3. Роли

Имена файлов не меняются. Шаблоны — готовые в `wp142-sketch/` (`coder.prompt.template.md`,
`reviewer.prompt.template.md`), оба проверены загрузкой.

### 3.1. `agents/coder/agent.toml`
`addons = [{ name = "context-budget", params = { cut = "pass" } }, { name = "dcg" }]`, комментарий про `dcg` — одна
строка. Остальное как есть. Аддоны прикрепляются **только** в `agent.toml` кодера и ревьюера, не в `[settings]`
проекта: у субагента с тем же аддоном, но другим `cut`, команда хука отличалась бы от хука сессии, и хук сработал бы
дважды (хуки сессии и так работают внутри субагентов).

### 3.2. `agents/coder/prompt.md` — переписать целиком (ориентир 110–130 строк)
1. Вступление: формула `epic`, шаги `code` (ты) → `tests` → `review` → `gate`; оператора нет.
2. Абзац про `CLAUDE.md` расширить: всё, что там о циклах, `loopctl`, `scripts/loop/`, `.loop/` (план, отчёты,
   журнал, handoffs), описывает другой луп; здесь задача — эпик, память — его комментарии, вопрос владельцу —
   комментарий и метка.
3. **The epic and its comments.** В промпте — описание эпика и последние комментарии в TOON, старше —
   `br comments <id>`. Комментарии — единственная память между раундами: итоги кодеров (`CODER`), ревью (`REVIEW`),
   дампы остановленных сессий, ответы владельца (автор не `loop-manager:…`). Дочерних бидов нет: `br create` не
   звать. Статус и исполнителя эпика не менять (их ведут демон и гейт); из меток — только `needs-owner`.
4. **Order of work.**
   1. Разбор: `git rev-parse HEAD` (запомнить как `BASE`), `git status --short`, `git log --oneline -15`, последние
      комментарии. Незакоммиченное дерево — недоделанная работа прошлого кодера по этому же эпику, она твоя:
      `git diff --stat`, затем довести и закоммитить либо откатить, сказав об этом в комментарии. Молча не выбрасывать.
   2. Последнее ревью: раздел «Обязательно до продолжения» — первая работа, по порядку. Ревью велит откатить и
      сделать иначе — так и сделать (`git revert`, историю не переписывать).
   3. Ответ владельца в комментариях — решение, ему следовать.
   4. Работа законченными частями, коммиты по шагам, сообщение начинается с `<id эпика>: `.
5. **Context.** Бюджет 300k, заметка попросит свернуться; пункты про обрезку вывода и субагентов — как сейчас.
   О жёстком пределе — ни слова.
6. **Subagents.** Таблица та же. Длинный результат — в `target/devloop/notes/<тема>.md`. План эксперта кодер кладёт
   комментарием в эпик (`br comments add "$LOOP_MANAGER_BEAD" -f <файл>`): следующему раунду файл не виден.
7. **Implementation.** Правила кода, тестов, `docs/`, решений и допущений — как сейчас (запись «в журнал» → `NOTE`-
   комментарий в эпик). Новое: на ревью отдаётся то, до чего дошёл раунд, зелёным его делать не обязательно; полный
   `mise run ci` не гонять — его гонит шаг `tests` сразу за тобой, на дереве, как ты его оставил; собирающееся —
   закоммитить, несобирающееся — оставить в дереве и описать в «Начато и не закончено». `devloop/` можно править как
   любой код репозитория (скрипты берутся из дерева в момент шага, конфиг — после `config reload` владельца).
   **Убрать:** пункт «`devloop/` is … the owner's: do not change it», отчёт `.loop/runs/…`, `BASE:` в отчёте, поиск
   прошлых отчётов `grep … .loop/runs`, пункт про `.loop/PLAN.md`, `.loop/JOURNAL.md`.
8. **The comment** — по-русски, файл в `target/devloop/`, затем `br comments add "$LOOP_MANAGER_BEAD" -f <файл>`:

   ```text
   CODER: done | partial | blocked
   BASE: <HEAD at the start of the round>
   HEAD: <HEAD at its end>
   ## Сделано (коммиты)
   ## Не вышло и почему
   ## Начато и не закончено (файлы, где остановился, следующий шаг)
   ## Замечания ревью: что закрыто, что нет
   ```
9. **A question to the owner.** Комментарий `QUESTION: …`, читаемый сам по себе (что решается, что на кону,
   варианты, какой принят до ответа), и `br label add "$LOOP_MANAGER_BEAD" needs-owner`; работа идёт дальше по
   принятому варианту. Находка или решение по допущению — комментарий `NOTE: …`.
10. **Finishing.** Остановить фоновое; закоммитить; комментарий; `loop-manager finish pass --summary "<done|partial>:
    …"`. `finish fail --summary "blocked: <why>"` — только когда без владельца нечего делать: вопрос задан
    (комментарий и метка), всё остальное сделано. Вопрос уже висит без ответа и другой работы нет — `fail` сразу.

### 3.3. `agents/reviewer/agent.toml`
`addons = [{ name = "context-budget" }, { name = "dcg" }]`. Субагентов нет, как сейчас.

### 3.4. `agents/reviewer/prompt.md` — переписать целиком (ориентир 100–110 строк)
1. Вступление и абзац про `CLAUDE.md` — как у кодера. Код и документацию не правит и не коммитит; пишет комментарий.
2. **The round.** Вход: эпик, последние комментарии, блок `<tests>` — итог полного прогона на дереве кодера. Гейт
   заново не гонять (лог — `target/devloop/ci.log`). Красный прогон и незакоммиченное дерево — не отказ от ревью:
   разобрать код и сказать, что дальше. Изменения раунда: от `HEAD:` прошлого `REVIEW` (в первом раунде — от `BASE:`
   итога кодера) до HEAD, плюс `git status --short` и `git diff`. Эпик целиком: `git log --oneline --grep='^<id>:'`.
3. **What to check** — нынешние пункты 2–7; пункт 8: итог кодера правдив. Пункт про `.loop/PLAN.md` убрать.
4. **Strictness** — нынешние 1–3, 5–7; **пункт 4 («Any change under `devloop/` … waits for the owner») убрать**.
   К мутациям добавить: в дереве с незакоммиченной работой кодера мутацию снимать обратной правкой, не `git checkout`
   и не `git stash`; `git diff --stat` до и после совпадает. Новый пункт: можешь велеть откатить и сделать иначе —
   назови коммиты и другой подход.
5. **The score.** `SCORE` — насколько сделан эпик, 0–10, один знак. 9 и выше и `VERDICT: complete` — только когда
   сделано всё, что просит описание, `<tests>` говорит `RESULT: pass` и `UNCOMMITTED: 0 paths`, обязательных пунктов
   нет. Иначе `continue`. Открытый вопрос владельцу, от которого зависит «готово», — `continue`.
6. **The comment** — по-русски, первые четыре строки точно так:

   ```text
   REVIEW
   SCORE: 7.5/10
   VERDICT: continue
   HEAD: <the commit reviewed>
   ## Обязательно до продолжения
   ## Желательно
   ## Проверено
   ## Нарушения правил
   ## Следующие шаги
   ```
   Комментарий самодостаточен: следующий раунд видит только последние комментарии, поэтому незакрытый пункт
   повторяется целиком, а не ссылкой на прошлое ревью. «Следующие шаги» — что брать следующему кодеру с учётом его
   бюджета; это и есть план эпика. Пункт, который ждёт владельца, так и помечается; вопрос владельцу — как у кодера
   (`QUESTION` и метка `needs-owner`).
7. **Finishing.** `loop-manager finish pass --summary "SCORE 7.5/10, VERDICT continue" --data '{"score": 7.5,
   "verdict": "continue"}'` — всегда `pass`, число — числом JSON. `finish fail` — только когда ревью написать не вышло.

### 3.5. Субагенты (`helper`, `subcoder`, `expert`) — остаются, правки точечные
- `helper/prompt.md`, `subcoder/prompt.md`: `.loop/runs/<run>/notes/` → `target/devloop/notes/`.
- `expert/prompt.md`: то же; «entry in `.loop/JOURNAL.md`» → «сказать в ответе, кодер запишет в эпик»; «the files of
  `.loop/` in Russian» → «план пишется по-русски: кодер кладёт его комментарием в эпик».
- Три `agent.toml` не меняются.

## 4. Что удалить и новый `gate.sh`

Удалить: `devloop/scripts/check-report.sh`, `devloop/scripts/check-review.sh` (`git rm`), оба
`step_check_script_path` и `max_attempts = 3` у `code`, `[exec] inherit_env = true` проекта, все упоминания
`.loop/runs/`, `report.md`, `review.md`, `.loop/JOURNAL.md` в паке, `git add .loop` в гейте.

`gate.sh` — `wp142-sketch/gate.sh`, 32 строки. Порядок:
1. `score`, `verdict` и `pass` — из `LOOP_MANAGER_STEP_REVIEW_DATA` через `jq`:
   `(.verdict? == "complete") and ((.score? | tonumber?) >= 9)`. Оценка строкой (`"9.5"`) принимается; нет данных, не
   объект, нет числа — «вердикта нет», гейт не пройден.
2. Не пройден → `br update "$LOOP_MANAGER_BEAD" -s rework -q` (сбой br — строка в вывод, не падение).
3. `br sync --flush-only -q`; `git add -- devloop/.beads`; коммит **только этого пути**
   (`git commit … -- devloop/.beads`), чтобы не забрать ни незакоммиченное, ни застейдженное кодером; `git push -q`
   с `GIT_TERMINAL_PROMPT=0` — после каждого ревью, как `loopctl`.
4. Выход: 0 — пройден и запушено; 75 — пройден, push не вышел (шаг повторяется политикой: три раза через 30 с, затем
   прогон падает); 1 — не пройден: строка `gate: SCORE …, VERDICT …`, при несостоявшемся push — ещё строка, и строки
   `RESULT|HEAD|LOG` из `LOOP_MANAGER_STEP_TESTS_DATA`, чтобы следующий кодер знал итог полного прогона.

Проверку «тесты зелёные» гейт сам не делает: в пакете условие — `complete` и оценка ≥ 9, зелёный прогон входит в
правило `complete` у ревьюера. `tests.sh` — `wp142-sketch/tests.sh`, 19 строк.

## 5. Self-test в `mise run ci`

Подключение уже есть и не меняется: в `mise.toml`, `[tasks.ci]`, строки `"devloop/scripts/selftest.sh"` и
`cargo run … config check --config devloop/config.toml`. Меняется сам скрипт — `wp142-sketch/selftest.sh`, 65 строк:
- `br` и `mise` подменены заглушками в `PATH` (`br` пишет аргументы в лог и трогает `issues.jsonl`, `mise` выходит с
  `MISE_RC`); `unset BEADS_DIR BD_ACTOR LOOP_MANAGER_STEP_*`. Это обязательно: `mise run ci` теперь идёт и внутри
  шага `tests` прогона пака, где в окружении настоящий `BEADS_DIR`, — без заглушки self-test писал бы в трекер пака;
- `gate.sh`: 9, 9.5, оценка строкой и `complete` → 0, запушено, `rework` не ставится; 8.9; 9 с `continue`; нет
  оценки; данные не объект; данных нет → 1 и `update dev-1 -s rework`; грязное дерево кодера остаётся как было, а
  трекер закоммичен и запушен; строка `RESULT:` тестов есть в выводе; push не вышел → 75 при проходе и 1 при возврате;
- `tests.sh`: зелёный и красный `mise` → выход 0, `RESULT: pass` / `RESULT: FAIL (mise run ci: exit 101)`, хвост
  лога при красном, счёт незакоммиченных путей, лог в `target/devloop/ci.log`.

## 6. README пака — разделы

1. Заголовок, абзац «что это», дерево каталога (`formulas/epic.toml`, `scripts/` — `tests.sh`, `gate.sh`,
   `selftest.sh`, `.beads/policy.yaml`).
2. **A run** — таблица шагов (`code`, `tests`, `review`, `gate`): что делает и что бывает, когда не прошёл; 50
   раундов; красные тесты раунд не тратят и к кодеру не возвращают; всё состояние — в комментариях эпика.
3. **The status of an epic** — таблица из 1.2, метки `auto` и `needs-owner`.
4. **Before the first start** — инструменты в `PATH` (`dcg` добавить), демон стартует из оболочки, где mise
   активировал репозиторий (иначе `config_error` на `RUSTUP_TOOLCHAIN`), `git push --dry-run`, ветка `main`; второй
   луп в том же checkout одновременно не работает.
5. **Start it** — как сейчас.
6. **File an epic** — `br create "<title>" -t epic -l auto --description-file <file>` (или `-d`); что писать в
   описании (документы, результат, проверяемое «готово»; длинная задача — ссылка на файл PRD в репозитории); запуск
   руками без метки.
7. **Watch it** — `run list|show|watch`, `events`; `br show <id>`, `br comments <id>`, `br list --status rework`,
   `br list -l needs-owner`; лог последнего прогона тестов — `target/devloop/ci.log`.
8. **Answer `needs-owner`** — `br comments add <id> "<ответ>"`, `br label remove <id> needs-owner`; агент следующего
   шага или раунда увидит ответ в промпте. Прогон упал с `blocked` — см. следующий раздел.
9. **When a run fails** — причины (кончились раунды, `blocked`, сбой инфраструктуры, push); эпик снова `open`, гейт
   `bead:devloop/<id>` закрыт; `loop-manager gate list`, `loop-manager gate open bead:devloop/<id>`; с меткой `auto`
   новый прогон уже стоит в `queued` и стартует сам, без метки — `run start`.
10. **The push** — как сейчас, но `inherit_env` теперь на шагах `tests` и `gate`; гейт коммитит только
    `devloop/.beads/`; закрытие эпика пишется в `issues.jsonl` уже после последнего push — оно уйдёт со следующим
    коммитом трекера или руками.
11. **The sessions** — что задаёт `[settings]` (тулчейн, TOON у br, кеш 5 минут и чем он задан), бюджет контекста,
    `dcg` (что нужно на хосте — ссылка на README аддона).
12. **The addons** — почему путь и на что сменить после релиза с тегом (1.3).
13. **Changing this directory** — вместо «Who may change this directory»: править можно и агентам; скрипты берутся
    из дерева в момент шага, конфиг и промпты — после `loop-manager config reload`. Абзац про закрепление пака из git
    — убрать.
14. **Moving over** — оба лупа одновременно не запускать (один checkout); шаги снятия старого лупа — как сейчас;
    пока `.claude/settings.json` на месте, его хуки работают в сессиях пака (что именно — из раздела 0); `br init`
    после свежего клона.

## 7. Проверка готовности

1. `cargo run -q --locked -p loop-manager -- config check --config devloop/config.toml` — exit 0, вывода нет. Сейчас
   на паке WP-134 команда чистая (`target/debug/loop-manager`, exit 0); на эскизе — тоже.
2. `sh devloop/scripts/selftest.sh` → `devloop selftest: ok`; по одной мутации в `gate.sh` (`>= 9` → `>= 8`) и
   `tests.sh` (`RESULT: pass` всегда) — self-test краснеет.
3. `loop-manager config explain --config devloop/config.toml --project devloop --formula epic --rig repo`: у `code` и
   `review` — три переменные тулчейна, `BR_OUTPUT_FORMAT`, `native.claude` с двумя TTL, оба аддона, у кодера
   `cut = "pass"`; у `tests` и `gate` — `exec.inherit_env = true`.
4. `grep -rn '\.loop\|check-report\|check-review\|waits for the owner' devloop/` — только место в README и ролях, где
   сказано, что `.loop/` относится к другому лупу, и пометка пункта ревью «ждёт владельца».
5. Контрактный тест `br` из шага Г зелёный; `mise run ci` зелёный (`scripts/loop/checks 136`).

## 8. Чего пак не может — в журнал

Кодер дописывает в `.loop/JOURNAL.md` (НАХОДКА, одной-двумя записями; каждая читается сама по себе):
1. **br**: политика статусов — в `.beads/policy.yaml`, не в `config.yaml`. `--transition-comment` нельзя приложить к
   обновлению без смены статуса, поэтому «комментарий возврата» на каждый раунд им не сделать; статус возврата
   ставит `gate.sh`, причина — в комментарии ревьюера.
2. **Статус `rework` виден ядру как текст.** Контракт 5.2 называл пять статусов — документ правится в этом пакете
   (шаг Г). После упавшего прогона демон ставит `open`: то, что эпик был в доработке, видно только по комментариям.
3. **Закрытие эпика не попадает в последний push**: демон закрывает бид после шага `gate`, шага «после закрытия» в
   продукте нет; `devloop/.beads/issues.jsonl` остаётся изменённым до следующего коммита трекера.
4. **`needs-owner` не останавливает запуск**: в `where` нет отрицания. Останавливает гейт бида при
   `bead_failed_runs = 1`; такой порог считает и сбой инфраструктуры (вопрос цикла 133, 12:10, остаётся открытым).
5. **В промпте нет номера раунда** (`RunView` без него), и кодеру не назвать `steps.tests.*` (шаг позже): итог
   тестов он получает из вывода гейта и комментария ревьюера.
6. **Вывод exec-шага — последние 4 KiB**: ревьюер видит выжимку, целый лог читает из `target/devloop/ci.log`.
7. **Вердикт без `--data` нельзя дозапросить в той же сессии** (повтор шага — новая сессия): гейт считает такой
   раунд возвращённым, следующий раунд ревьюится заново.
8. **Потолок 20 сбойных попыток — на прогон, а не на раунд** (инвариант 16): прогон из 50 раундов делит его на все.
9. **Хуки старого лупа работают в сессиях пака**, пока лежит `.claude/settings.json`: субагент получает вторую
   заметку о контексте, и она называет жёсткий предел 420k, а на 420k старый хук сам останавливает субагента с
   дампом в `.loop/handoffs/`; `env` оттуда же задаёт глубину субагентов 4 и может перекрыть 2 из пака (не
   проверено). Снять нельзя, не трогая `.claude/`.
10. **Конфиг и промпты пака демон держит в памяти** до `config reload`: правка агентом `devloop/*.toml` или роли
    начинает действовать только после перезагрузки владельцем; скрипты — сразу.
11. **Аддоны не закреплены на тег**: тега с новым `context-budget` и с `dcg` нет (`v0.0.1` — версия 0.1.0 со
    сжатиями). После релиза — `git` + `rev` + `dir`, README говорит как.
12. **Кеш промптов**: у claude задан ключами настроек, проверить без живой сессии нельзя; у codex 0.170.0 настройки
    TTL не найдено.

Отдельной записью — ВОПРОС: «кеш 5 минут» прочитан как «ставим 5 минут, потому что долгий прогон тестов вынесен из
сессий отдельным шагом и кеш на нём не теряется». На кону: при 5 минутах пауза в сессии дольше 5 минут (холодная
сборка, долгий субагент) стоит повторного чтения контекста по полной цене. Если владелец имел в виду «кеш должен
переживать долгие паузы» — это одна правка: оба ключа в `"1h"`. Принято 5 минут.

## 9. Шаги реализации и контекст

Пакет помещается в одну сессию: рабочий объём ≈ 115–130k. Порядок такой, чтобы после каждого шага `config check`
был чистым.

| Шаг | Что | Файлы | Контекст |
|---|---|---|---|
| А | Конфиг, формула, шаблоны, скрипты, self-test — перенос эскизов | `devloop/config.toml`, `loop-manager.toml`, `formulas/epic.toml` (`git mv` из `task.toml`), `.beads/policy.yaml`, `agents/{coder,reviewer}/{agent.toml,prompt.template.md}`, `scripts/{gate,tests,selftest}.sh`, `git rm scripts/check-*.sh` | 20–25k |
| Б | Роли | `agents/coder/prompt.md`, `agents/reviewer/prompt.md`, три `prompt.md` субагентов | 35–45k |
| В | README пака | `devloop/README.md` | 20–25k |
| Г | Продукт: статус — текст | `docs/design/contracts.md` 5.2 (одна строка), `crates/loop-manager-contract/src/tracker.rs:15` (rustdoc), `docs/design/tracker-br.md` (пункт в «Limits of br…»), тест в `crates/loop-manager-br/tests/contract.rs` | 15–20k |
| Д | Решение, журнал, план, гейт | `docs/agents/decisions.md` (строка 150), `.loop/JOURNAL.md`, `.loop/PLAN.md`, `scripts/loop/checks 136` | 10–15k |

- **А**: `sh devloop/scripts/selftest.sh` и `config check` — сразу после переноса.
- **В** можно отдать `subcoder` параллельно с **Б**: файлы не пересекаются, бриф — раздел 6 и разделы 0–1 этого плана.
- **Г**. Строка 5.2: `status` — статус br текстом: `open`, `in_progress`, `blocked`, `deferred`, `closed` или тот,
  что добавляет политика трекера проекта (`rework`); ядро по нему ничего не решает. `tracker-br.md`: статус, который
  объявляет `.beads/policy.yaml`, проходит как текст; захват ставит `in_progress`, отпускание — `open`, закрытие
  идёт из любого статуса. Тест (15–25 строк, рядом с `release_and_close_change_only_a_bead_their_owner_holds`):
  в базе с `policy.yaml` из пака (`strict` + шесть статусов) захват → `br update -s rework` → `release` даёт `open`
  без исполнителя; захват → `rework` → `close` даёт `closed`. Пак стоит на этом поведении, без теста фраза в
  документе ничем не держится.
- **Д**. Решение 150, раздел «Process»: `loop-manager's own development runs on loop-manager as the pack `devloop/`:
  the whole task is one epic bead, with no child beads; a round is a coder, the full test run and a reviewer; the
  coder hands over what it reached, red tests included, and the test run is a step whose result goes to the reviewer,
  not a check; each round is a new coder that reads the epic and its comments, which hold all the state; an epic may
  take many rounds (50), and running out of them is a failure that waits for a human; a question to the owner is a
  comment on the epic and the label `needs-owner`; the agents may change `devloop/` (2026-11-19)`.
- **Если контекста не хватает** — резать после А+Г (пак грузится, CI зелёный, отчёт `partial`): Б, В, Д — следующим
  циклом. Между ними роли ещё описывают старый процесс, запускать пак в этом промежутке нельзя — сказать в отчёте.

## 10. Допущения expert

- Эпик заводится типом `epic` (`-t epic`), как сказано в запросе; формуле тип не важен.
- `prefix = "lm"` у рига не трогаю: дочерних бидов нет, на что он влияет — только на `br create` из сессии.
- `git push -q` пушит текущую ветку в её upstream, как в паке WP-134; `loopctl` пушит `HEAD:main` — README требует
  ветку `main`.
- `docs/guide/development.md` (строка про self-test пака и загрузку его конфига) и `mise.toml` остаются верными.

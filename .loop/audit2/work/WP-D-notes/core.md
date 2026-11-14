# WP-D, группа core: §5.3, §5.4, §9.2, §9.3, §10.3, §11.1, §11.2, §11.4 (+ таблица §9.4 в contracts 6.3)

Текущая спека по §5.3–§11.4 совпадает с оригиналом, кроме правок владельца в §10.1 (п. 14) и §11.4 (п. 4–5).

### §5.3 — сжимать: да; строк в текущей спеке: 13

| требование | где в docs/ | дописано мной |
|---|---|---|
| JSONL, ротация 10 МБ × 5 [Р] | — | event-model.md «Journal» (сверено: `journal.rs:55-57`) |
| канал наблюдения, не источник истины; 50 МБ | — | event-model.md «Journal» |
| один журнал на оркестратор [Д] | — | event-model.md «Journal» (`daemon.rs:70`) |
| конверт: seq (один счётчик), схема, время, тип, проект, риг, прогон, correlation, нагрузка | contracts.md 1.1 | ссылка из «Journal» |
| чтение с seq; вытеснен — явный «разрыв», восстановление через API | contracts.md 3.5 (`:284`), frontends.md:23-24 | event-model.md «Journal» |
| только события оркестрации; разговор ядро не хранит [Д]; живой просмотр — ретрансляция; история — транскрипт | frontends.md:25-29, contracts.md 4 (live tap) | event-model.md «Journal» |

MEASURE: нет. [НП]: нет.

```text
Реализовано: JSONL-журнал, ротация по размеру 10 МБ × 5 файлов **[Р]**; канал наблюдения, а не источник истины
(истина — состояние); один журнал на оркестратор для всех проектов; `seq` — один счётчик на оркестратор; чтение
продолжается с `seq`, вытесненный `seq` — явный «разрыв» и восстановление через API; содержимое разговора ядро не
хранит: живой просмотр — прямая ретрансляция потока адаптера, история — транскрипт харнесса. Код:
`crates/aae-core/src/journal.rs`, `crates/aae-front/src/api/events.rs`, `api/live.rs`. Документация:
`docs/design/event-model.md` («Journal»), `docs/design/contracts.md` 1.1, 3.5, `docs/api/frontends.md`.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §5.4 — сжимать: да; строк: 23

| требование | где в docs/ | дописано мной |
|---|---|---|
| список наблюдаемых событий | contracts.md 1.3 (таблица Observed) | заголовок таблицы: «читают, не влияют» |
| пять решающих событий, что может хук | contracts.md 1.3 (нагрузка), plugin.md:94-100 (решения) | contracts.md 1.3: таблица «Type / What a hook may do / No plugin decision» |
| колонка «без решения плагина», в т. ч. при выключенном default-policy | contracts.md 3.3 (текстом) | та же таблица; сверено с `run/decide.rs:59-113`, `decide_deadline` |
| каждый отказ действовать — событие с причиной, молчаливых ожиданий нет | — | contracts.md 1.3, абзац после таблиц |

Поправлено: config.md («Where the bundled plugins end…») — ссылка на колонку теперь ссылка на contracts 1.3.

Расхождения: нет. Уточнение в доке по коду: на `run.requires_action` без решения после `human_timeout` — `deny` и
возврат в `running` (не конец прогона); на `attempt_failed`/`turn_ended` причина `no_decision`. В трактовке
«отказ → событие» добавлено, что отказ API-запроса — ответ-ошибка, не событие (так в коде).

```text
Реализовано: наблюдаемые события (плагины и клиенты читают, не влияют) и пять решающих (`run.preparing`,
`run.requires_action`, `run.attempt_failed`, `run.turn_ended_without_outcome`, `run.finishing`) с таблицей «что может
хук / что делает ядро без решения плагина» (в том числе при выключенном `default-policy`); каждый отказ ядра
действовать пишет событие с причиной, молчаливых ожиданий нет. Код: `crates/aae-core/src/run/decide.rs`,
`run/hooks.rs`. Документация: `docs/design/contracts.md` 1.3 (нагрузки и таблица решающих событий), 3.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §9.2 — сжимать: да; строк: 15

| требование | где в docs/ | дописано мной |
|---|---|---|
| нет тестового режима; одна исполняемая acceptance spec [Р] | README.md «Acceptance spec» (частично) | development.md «Acceptance scenarios», первый абзац |
| сценарии реализуют критерии §15 | README.md:69-70 | там же |
| путь рабочих прогонов: настоящий демон и бинарники харнессов | README.md:69 (демон) | там же (+бинарники) |
| дешёвая модель из фикстур агентов | — | там же (haiku; сверено `scenarios/c04a_background.rs:22`) |
| полный прогон только по явной команде, автоматически никогда | development.md:16, :22-24 | там же |
| безмодельные — те же сценарии; CI на каждый коммит; колонка «Запуск» §15 | development.md:14, README.md:70 | там же |
| отчёт с датой, версией aae и харнессов | README.md:76-77 | там же |

```text
Реализовано **[Р]**: отдельного тестового режима нет — проверка формул и проверка самого aae одна исполняемая
acceptance spec (`crates/aae-acceptance`); сценарии идут путём рабочих прогонов (настоящий демон и бинарники
харнессов); сценарии с моделью — на дешёвой модели из фикстур, только по явной команде (`mise run acceptance`),
никогда автоматически; безмодельное подмножество — те же сценарии, CI на каждый коммит (`mise run acceptance-ci`);
отчёт с датой и версиями aae и харнессов. Документация: `docs/guide/development.md` («Acceptance scenarios»),
`README.md` («Acceptance spec»).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §9.3 — сжимать: да; строк: 25

| требование | где в docs/ | дописано мной |
|---|---|---|
| граф состояний | contracts.md 6.1, 6.2 | — |
| классы провала и их смысл | 1.3 (`run.failed`), invariants 13 | contracts.md 6.1, список после `interrupted` |
| queued vs parked | 6.1 (таблица) | 6.1, список |
| риг из входа/триггера/метки; нет рига — ошибка до запуска [Д] | — | 6.1: риг при создании или триггером; неизвестный — отказ до создания |
| рабочий каталог — один живой прогон, второй в queued; параллельно — worktree [Д] | 6.2 («workdir free»), 1.3 `waiting_for: workdir` | 6.1, список |
| попытка: новая сессия или resume, своё поколение | 1.3 Attempts, 6.2 | 6.1, список |
| параллелизм: оркестратор и агент [Д] | config.md (`max_runs` ×2) | 6.1, список |
| явное продолжение сессии [Р], неявного нет | 6.2 (`continue_from`) | формулировка без «§9.3» |

Расхождение спека ↔ код (не чинил): спека — «риг берётся из входа, из триггера или из метки бида»; в коде риг
обязателен при создании (`run/queue.rs:16-17`) или даётся триггером; метка бида только проверяется (бид другого
рига отвергается), риг из неё не выводится. Док описывает код.

```text
Реализовано: машина состояний прогона (`queued → preparing → starting → running ⇄ requires_action → finishing →
passed | failed{класс} | cancelled`, плюс `parked` и `interrupted`) с дедлайном у каждого состояния; классы провала
`agent | check | hollow_done | infrastructure`; риг задаётся при создании или триггером; рабочий каталог занят одним
живым прогоном (параллельно — `worktree = true`); параллелизм на оркестратор и на агента; у попытки своё поколение;
продолжение сессии другого прогона — только явно, `continue_from` **[Р]**. Код: `crates/aae-core/src/run/`.
Документация: `docs/design/contracts.md` 6.1–6.2.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §9.4 — не моя секция для сжатия (не сжимаю); перенесена только таблица

Таблица поведения финишера перенесена в contracts.md 6.3 («How the finisher behaves»), сверена с
`run/finish.rs:28-60`, `:88-104`; заодно туда же: финишер — слово агента (pass/fail, резюме, данные), сбой
инфраструктуры — не исход, данные доступны триггерам как `outcome`, «pass — слово агента, плагина проверки в
комплекте нет». Ссылка «The table of §9.4 maps onto 6.2» заменена.

### §10.3 — сжимать: да; строк: 25

| требование | где в docs/ | дописано мной |
|---|---|---|
| бид — вход; прогоны без бида [Р] | first-formula.md:103-110, config.md (bead input) | tracker-br.md «Beads and runs» |
| claim/возврат/закрытие — демон [Р], весь прогон, passed закрывает, failed/cancelled — условно | contracts.md 5.1, 6.2, invariants 2-5 | tracker-br.md |
| цепочки формул нет, pass закрывает [Д]; следующий — штатным подбором, в т. ч. в эпике [Р] | contracts.md 6.2 | tracker-br.md |
| «следующий готовый», выбор+claim одной операцией, «нечего брать» [Д] | contracts.md 6.2 | tracker-br.md |
| один живой владелец, второй — отказ с причиной | contracts.md 6.2 | tracker-br.md |
| счётчик неудач бида → ворота, событие [Д] | contracts.md 6.2, config.md (`bead_failures`) | tracker-br.md |
| владение в состоянии — истина; сбой close/release не меняет исход, повтор, расхождение событием | contracts.md 5.2, 6.2 | tracker-br.md |
| агент сам пользуется br для содержательной работы | — | tracker-br.md |
| §2.1 п. 14: явный бид с меткой `rig:` другого рига отвергается | config.md (bead input) | tracker-br.md (сверено `run/beads.rs:23-29`, `:132-134`) |

tracker-br.md теперь самодостаточен: «spec §10.3» во введении заменён ссылкой на «Beads and runs».

```text
Реализовано: бид — вход прогона, прогоны без бида разрешены **[Р]**; claim, условный возврат и закрытие делает
демон **[Р]**, claim держится весь прогон, `passed` закрывает бид, `failed`/`cancelled` возвращают условно; работа
режется на биды, следующий берётся подбором «следующий готовый», в том числе в эпике **[Р]** — выбор и запись
владельца одной транзакцией, нет готовых — явное «нечего брать»; цепочки формул над одним бидом в первом выпуске нет,
`pass` закрывает бид (цепочка не критична **[Р]**); бид с меткой `rig:` другого рига отвергается (§2.1 п. 14);
счётчик неудач бида закрывает ворота `bead:` (`bead_failures`); владение в состоянии — истина, br — отражение.
Код: `crates/aae-core/src/run/beads.rs`, `run/queue.rs`. Документация: `docs/design/tracker-br.md` («Beads and runs»),
`docs/design/contracts.md` 5, 6.2.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §11.1 — сжимать: да; строк: 11

| требование | где в docs/ | дописано мной |
|---|---|---|
| плагин — единица расширения, типовые расширения — плагины, не код ядра [Р] | — | config.md «Plugin», абзац после примера |
| каталог: plugin.toml (имя, версия, параметры, вклад, хуки, сервисы), skills/ hooks/ bin/ | plugin.md:1-29, config.md «Plugin» | config.md «Plugin» (состав каталога) |

```text
Реализовано **[Р]**: плагин — единица расширения, типовые расширения делаются плагинами, а не кодом ядра; плагин —
каталог с `plugin.toml` (имя, версия, параметры, вклад в агентов, хуки оркестратора, сервисы) и своими `skills/`,
`hooks/`, `bin/`. Документация: `docs/guide/plugin.md`, `docs/design/config.md` («Plugin»); пример —
`examples/aae-plugin-review`.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §11.2 — сжимать: да; строк: 10

| требование | где в docs/ | дописано мной |
|---|---|---|
| вклад: skills, хуки харнесса, MCP, env, скрипты в PATH, фрагменты промпта, родные части | plugin.md:56-74, config.md «Plugin» | — |
| только подключившие агенты [Р] | plugin.md:45-53 | config.md «Plugin» (явно, с `native.<harness>`) |
| хуки харнесса — диалект Claude | plugin.md:63-64, config.md (таблица settings) | — |
| процессная часть → отдельный экземпляр | config.md «Plugin» (PATH → fingerprint), contracts.md 8 | config.md «Plugin» (общее правило) |

Расхождение спека ↔ код: спека — «claude и codex исполняют диалект нативно»; plugin.md:63-64 — для codex aae
хуки транслирует. Не проверял по коду глубже; документ оставлен как есть.

```text
Реализовано: сторона агента декларативна — skills, хуки харнесса (диалект Claude), MCP, env, `bin/` в `PATH`,
фрагмент промпта, `native.<harness>`; вклад получают только агенты, подключившие плагин **[Р]**; вклад в
процессную часть конфига даёт этим агентам свои экземпляры по отпечатку. Код: `crates/aae-core/src/config/assemble.rs`,
`config/files.rs`. Документация: `docs/guide/plugin.md` («The agent side»), `docs/design/config.md` («Plugin»).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

Заметка: «хранилища для плагинов в ядре нет» — это текст §11.3 (не моя), дописан в config.md «Plugin» вместе с
«хуки без состояния, событие несёт счётчики» — §11.3 у соседа может на это сослаться.

### §11.4 — сжимать: да; строк: 42

| требование | где в docs/ | дописано мной |
|---|---|---|
| сервис: команда, область, здоровье, тот же ленивый реестр | config.md «MCP servers and services», contracts.md 8 | — |
| форма `[[service]]`, MCP по желанию, «только MCP» сохраняется [Р] | config.md:55-66 | — |
| мост stdio→HTTP — тот же сервис, сделан, в первом выпуске [Р] | config.md:45-52 (`aae mcp-bridge`) | — |
| общий MCP один на область, заголовки идентичности [Р], опыт 2026-11-10 | config.md:42-43 (заголовки) | config.md: пункт «One server per scope» + MEASURE |
| обе эпохи протокола; codex 0.166 только 2025-06-18 | mcp.md «Protocol epochs», config.md:47 (мост) | config.md: пункт про эпохи |
| быстрый рестарт; claude 7–26 с, 1 из 4 упал | — | config.md: пункт + MEASURE |
| сбой MCP — сбой инструмента | config.md:85 | config.md (повтор в пункте) |
| stdio с состоянием на клиента — scope session | config.md:50-52 | — |
| loopback без аутентификации — unix-сокет или токен | config.md (token, `AAE_SERVICE_TOKEN`) | — |

MEASURE перенесены: `shared-http-mcp` → docs/design/config.md («MCP servers and services»);
`claude-mcp-reconnect` → docs/design/config.md (там же). Устаревшей фразы «Не сделано: …моста нет» в текущей
спеке уже нет.

```text
Реализовано **[Р]** (§2.1 п. 4–5): `[[service]]` в конфиге или `plugin.toml` — долгоживущий процесс под супервизией
в том же ленивом реестре, что экземпляры харнессов; область оркестратор | проект, проверка здоровья, MCP по
желанию (вариант «только MCP» сохранён); stdio-MCP с областью не «сессия» — за мостом stdio→HTTP (`aae mcp-bridge`,
в первом выпуске **[Р]**). Общий MCP один на область **[Р]** с заголовками идентичности, обе эпохи протокола,
быстрый рестарт, сбой MCP — сбой инструмента, сервис на loopback только с токеном. Код:
`crates/aae-core/src/config/service.rs`, `run/services.rs`, `run/service_proc.rs`, `crates/aae/src/bridge.rs`.
Документация: `docs/design/config.md` («MCP servers and services»), `docs/design/contracts.md` 8, `docs/api/mcp.md`.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

## Изменённые мной файлы docs/

- `docs/design/event-model.md` — новый раздел «Journal».
- `docs/design/contracts.md` — 1.3 (таблица решающих событий, абзац об отказах), 6.1 (классы, риг, каталог,
  параллелизм, попытки), 6.3 (таблица финишера и смысл исхода); ссылки в 2.3, 4.3, 5.2, 6.1, 6.2, 7, 8.
- `docs/design/tracker-br.md` — введение, новый раздел «Beads and runs».
- `docs/guide/development.md` — абзац §9.2 в «Acceptance scenarios».
- `docs/design/config.md` — точечно: MCP (3 пункта + 2 MEASURE), «Plugin» (абзац), ссылка на contracts 1.3, убраны
  «§11.2» в таблице settings и в примере plugin.toml.
- `docs/api/mcp.md` — убрано «(spec §11.4)».

## Ссылки на секции группы

Заменены: contracts.md (§10.3 ×4, §5.3 в 4.3, §9.3 ×2, §9.4, §11.4, §5.4 ×2 в 1.3), tracker-br.md (§10.3),
config.md (§11.2 ×2), mcp.md (§11.4).

Остались в docs/ (чужие зоны):
- `docs/design/contracts.md` 3.3 (~:242): «"no plugin decision" column of §5.4» → заменить на «column of 1.3».
- `docs/design/contracts.md` 3.5 (~:284): «read the journal from a `seq` (§5.3)» → «(event-model.md, Journal)».
- `docs/design/config.md:41` — заголовок «MCP servers and services (spec §11.4)»: якорь
  `#mcp-servers-and-services-spec-114` используют `docs/guide/plugin.md:74` (чужой) и contracts.md 8; переименование
  заголовка — вместе с этими ссылками.

В `crates/` (не трогал), 53 места: `aae-core/src/journal.rs:1,198`, `driver.rs:300`, `store.rs:132`,
`tracker.rs:1,3`, `orphans.rs:135`, `config/{assemble.rs:151, files.rs:42,66, load.rs:18,22, mod.rs:24,26,45,
service.rs:1}`, `run/{mod.rs:1, msg.rs:194, beads.rs:1,20,194, finish.rs:1, hooks.rs:25, services.rs:1,75,110,
service_proc.rs:1}`, `tests/config_services.rs:1`; `aae/src/bridge.rs:1`, `aae/tests/{tracker.rs:1, mcp.rs:1,
bridge.rs:1}`; `aae-front/src/api/{events.rs:1, live.rs:1, mcp.rs:2, runs.rs:34,39}`, `aae-front/src/cli/finish.rs:1`;
`aae-testkit/tests/{finisher.rs:1,196, services.rs:1, bead_events.rs:1}`; `aae-acceptance/src/main.rs:1`,
`scenarios/c10_quota.rs:26`. Полный список: `grep -rnE '§(5\.3|5\.4|9\.[234]|10\.[13]|11\.[124])\b' crates`.

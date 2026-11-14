# WP-D, группа «harness»: §7.1–7.5, §7.7, §16

Оригинал сверен с текущей спекой: секции 7.1–7.7 не менялись; §16 получил правку по §2.1 п. 1 (песочница отложена) и
п. 17 (лимит 5 000 строк). Учтены решения §2.1 п. 1, 3, 8, 17 и решение владельца о линиях харнессов в `mise.toml`
(JOURNAL, цикл 30, п. 4).

### §7.1 — сжимать: да; 31 строка (389–419)

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| семантика ACP v2: running/idle/requires_action, типизированная ошибка хода [Р] | contracts.md 4.3 (`state`), 4.4 | — |
| операции по экземпляру / сессии / транскрипту | contracts.md 4.1 (трейт), 4.2 | — |
| нормализованные события с seq | contracts.md 4.3 | — |
| 17 видов сбоя, кто порождает | contracts.md 4.4 | — |
| ожидание человека и фон вне хода — состояния, не сбои | — | contracts.md 4.4, первый абзац |
| ссылка на сессию непрозрачна; у codex транскрипт в CODEX_HOME | contracts.md 4.2 `SessionRef` | переписал скобку без «§7.1» |
| способности: сессий на процесс, время сброса, ретраит ли сам | contracts.md 4.5 | — |
| нет steer, смены модели в живой сессии, подключения человека [Д] | — | contracts.md 4.1, абзац под «Shape» |

MEASURE: нет. [НП]: нет.

```text
Реализовано: семантика ACP v2 — состояние `running | idle | requires_action` и типизированная ошибка хода **[Р]**;
один object-safe трейт драйвера (экземпляр: split и отпечаток, старт, здоровье, стоп; сессия: старт, resume по
непрозрачной ссылке, промпт с ключом идемпотентности, отмена, ответ на запрос, закрытие; транскрипт: проверка и живой
отвод), нормализованные события с seq, 17 видов сбоя (5 порождает ядро), способности адаптера. В интерфейсе нет steer
посреди хода, смены модели в живой сессии и подключения человека **[Д]**. Код: `crates/aae-core/src/driver.rs`,
`crates/aae-core/src/driver/failure.rs`. Описание — `docs/design/contracts.md` (4.1–4.5).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §7.2 — сжимать: да; 11 строк (420–430)

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| ловушки «успешного провала» (claude success+is_error; выход 0 без итога) | contracts.md 4.4 (последний абзац); harness-updates.md | — |
| граница хода — из авторитетного сигнала, не из ответа на запрос | contracts.md 4.6 (строка Turn end) | contracts.md 4.6, абзац под таблицей |
| ретраит ли харнесс сам; поверх claude не повторять, за codex — нужно | contracts.md 4.5 (было неточно) | contracts.md 4.5: claude до 10 попыток, `CLAUDE_CODE_MAX_RETRIES`; codex — 429 только с Retry-After, один раз |
| своя группа процессов на сессию/экземпляр | experiments.md (1b), invariants.md 7 | contracts.md 4.6, абзац под таблицей |
| диапазон версий, вне его предупреждение | contracts.md 4.5 `versions`; harness-updates.md п. 5 | — |

MEASURE: `claude-self-retries` → `docs/design/contracts.md` 4.5.

Расхождение doc ↔ код (исправил doc): contracts 4.5 говорил «codex false»; код (`crates/aae-codex/src/stream.rs:289`)
ставит `retried_by_harness = true` для `overloaded` и для `rate_limited`, если codex уже повторял (был `Retry-After`).

```text
Реализовано: адаптер закрывает ловушки «успешного провала» (claude `success` при `is_error:true`; выход 0 без итога хода —
`harness_crash`), берёт границу хода из авторитетного сигнала харнесса, помечает `retried_by_harness` (поверх claude
политика не повторяет; codex повторяет 429 только при `Retry-After`, без него повтор — за политикой), запускает каждую
сессию или экземпляр в своей группе процессов, объявляет проверенный диапазон версий (вне его предупреждают демон и
`aae doctor`). Код: `crates/aae-claude/src/stream.rs`, `crates/aae-codex/src/stream.rs`. Описание —
`docs/design/contracts.md` (4.4–4.6), `docs/harness-updates.md`.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §7.3 — сжимать: да; 20 строк (431–450)

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| ключ — отпечаток (харнесс + процессная часть) [Р] | contracts.md 8 (1-й пункт), 4.2 `Split` | — |
| ленивый старт, переиспользование, стоп по простою | contracts.md 8 (Acquire/Release) | — |
| плагин, меняющий процессную часть, → другой экземпляр; плагин оркестратора — нет [Р] | — | contracts.md 8, 1-й пункт |
| предел сессий; сверх — второй экземпляр [Д]; причина — падение роняет все сессии | contracts.md 4.5, 8 Acquire | contracts.md 8, 2-й пункт (причина) |
| тот же реестр для сервисов | contracts.md 8 (Services) | — |
| «процессное» решает адаптер; таблица claude/codex | contracts.md 4.6 (Process and session); experiments.md (опыты 2, 4–5) | contracts.md 8, 1-й пункт |
| claude: процесс на сессию, общего сервера и экономии памяти нет | contracts.md 4.5, 4.6 | contracts.md 8 |
| codex: память app-server (замер 0.166.0) | experiments.md (опыт 2, 0.167.2) | experiments.md: числа 0.166.0 + маркер |
| opencode — [НП] | остаётся в §7.6 (не сжимается) | — |

MEASURE: `codex-appserver-rss` (date 2026-11-10, ver 0.166.0) → `docs/design/experiments.md`, раздел codex
app-server, рядом с числами 0.166.0. Внимание: там уже был маркер с тем же id (2026-11-12, 0.167.2) — теперь два
маркера с одним id и разными date/ver; `scripts/budget.sh` уникальность id не проверяет.

Закрытые [НП]: хуки codex на тред — `docs/design/experiments.md:187` («Hooks per thread | Work»).

```text
Реализовано: ключ экземпляра — отпечаток (харнесс + «процессная» часть эффективного конфига) **[Р]**; ленивый старт,
общий экземпляр для всех сессий с тем же отпечатком, остановка по простою; плагин, меняющий процессную часть, сам даёт
другой экземпляр, плагин только стороны оркестратора экземпляров не плодит **[Р]**; предел сессий на экземпляр, сверх
него — второй с тем же отпечатком **[Д]**; тот же реестр обслуживает сервисы. Процессную часть определяет адаптер:
claude — процесс на сессию; codex — app-server на отпечаток (`CODEX_HOME`, `HOME`, skills, провайдер, окружение), на
тред — cwd, модель, effort, инструкции, MCP, хуки. opencode — §7.6. Код: `crates/aae-core/src/run/registry.rs`.
Описание — `docs/design/contracts.md` (8, 4.5, 4.6); память codex — `docs/design/experiments.md` (опыт 2).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §7.4 — сжимать: да; 30 строк (451–480)

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| транспорт `claude -p` stream-json, ack, `--session-id`, resume по id | contracts.md 4.6 (таблица); experiments.md 1a | contracts.md 4.6 «claude», Transport |
| конфиг на сессию флагами (источники настроек, settings с хуками, MCP, plugin-dir, промпт, модель, effort, режим) | experiments.md «Config and credential delivery» | contracts.md 4.6 «claude» |
| сигналы: api_retry, rate_limit_event, result+is_error, смена состояния, heartbeat 30 с | experiments.md 1b | contracts.md 4.6 «claude», Signals |
| модель при resume явно | experiments.md:42; contracts 4.6 Resume | — |
| время сброса — только при подписке; API-ключ — rate_limit без сброса | contracts.md 4.5; harness-updates.md | contracts.md 4.6 «claude» |
| форма события при настоящем исчерпании квоты [НП] | READINESS.md:25 | contracts.md 4.6 «claude» (открыто) |
| «ход закончен» = idle без фона; субагенты фоновые | contracts.md 4.6; experiments.md:111 | — |
| init после промпта; ждать init — взаимоблокировка | experiments.md 1a | contracts.md 4.6 «claude» |
| итогов хода ≠ промптов | experiments.md:111 (таблица) | contracts.md 4.6 «claude» |
| heartbeat нет из субагентов и при ожидании разрешения → сторож | invariants.md 15 (частично) | contracts.md 4.6 «claude» |
| смерть демона: ход дорабатывает, исход — из транскрипта и репо | contracts.md 6.4, harness-updates #11 | contracts.md 4.6 «claude» |
| холодный старт 0,25–0,35 с, тёплый пул не нужен | — | contracts.md 4.6 «claude» + маркер |

MEASURE: `claude-tool-heartbeat` → `docs/design/experiments.md` («Heartbeat of a long tool», рядом с `-1b`);
`claude-coldstart-init` → `docs/design/contracts.md` 4.6.

Закрытые [НП]: сброс модели после resume — `docs/design/experiments.md:42`; два сигнала idle — `experiments.md:111`.
Открыт: форма события при настоящем исчерпании квоты.

Расхождение спека ↔ код (не чинил, документировано): спека требует строгий режим MCP (`--strict-mcp-config`); код
(`crates/aae-claude/src/part.rs:160-199`) его не ставит, причина — `experiments.md`, «Consequences» (он отрезал бы
`.mcp.json` рига). Системный промпт — `--append-system-prompt`.

```text
Реализовано: `claude -p` со stream-json, `--session-id`/`--resume`; конфиг на сессию — флаги и файлы в каталоге
состояния, полный набор (и модель явно) на каждом resume; приём промпта — `command_lifecycle{queued}`; «ход закончен» —
idle без фона по правилу трёх условий; heartbeat инструмента раз в 30 с (его нет изнутри субагентов и при ожидании
разрешения — учитывает сторож); `init` до промпта не ждём; тёплого пула нет. Время сброса лимита — только при логине
подпиской. Открыто **[НП]**: форма события при настоящем исчерпании квоты (claude-половина критерия 10). Код:
`crates/aae-claude/`. Описание — `docs/design/contracts.md` (4.6, «claude»), опыты — `docs/design/experiments.md`
(1a, 1b, 4–5).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §7.5 — сжимать: да; 28 строк (481–508). Секция в спеке устарела (unix-сокет, генерация типов) — выжимка по коду и §2.1

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| собственный app-server; общий демон codex пользователя не используется | experiments.md опыт 2 (stdio выбран); contracts.md 4.6 | contracts.md 4.6 «codex» (про демон пользователя) |
| транспорт [НП] → stdio | experiments.md:184 | — |
| типы — не генерируются (§2.1 п. 8) | experiments.md «Types» противоречил | experiments.md «Types» переписан; contracts.md 4.6 «codex» |
| сигналы: error/willRetry, turn/completed, thread/status/changed, rateLimits | contracts.md 4.5 (частично) | contracts.md 4.6 «codex», Signals |
| 429 без Retry-After не ретраит; с ним — один раз | harness-updates.md #1 | contracts.md 4.5 |
| граница хода — только turn/completed; idle раньше; фон при idle; молчание провайдера без нотификаций и таймаута | contracts.md 4.6 Turn end; harness-updates.md #14 | contracts.md 4.6 «codex» |
| MCP на тред (stdio) [НП] | experiments.md:295 | — |
| два turn/start сливаются в steer; ввод сериализует демон | — | contracts.md 4.6 «codex» |
| [experimental], релизы каждые 2–8 дней; линия + контрактные тесты | harness-updates.md (линия) | harness-updates.md, вводная + маркер |
| хуки с окружением процесса; доставка хуков в тред [НП] | experiments.md:187 | contracts.md 4.6 «codex» (ссылка) |
| песочница [НП] | config.md:150, first-formula.md:34, experiments.md:246 | experiments.md таблица опытов (п. 6 отложен) |

MEASURE: `codex-release-cadence` → `docs/harness-updates.md` (вводная).

Закрытые [НП]: транспорт — `experiments.md:184`; хуки на тред — `experiments.md:187`; stdio-MCP на тред —
`experiments.md:295`. Открыт: песочница `workspace-write` (отложена, §2.1 п. 1).

Проверка «слияния turn/start»: ядро шлёт следующий промпт только после конца хода (`finishing` наступает после
idle без фона, `crates/aae-core/src/run/finish.rs:90`) — записал как гарантию в contracts.md 4.6; отдельного теста на
это не искал.

```text
Реализовано: собственный `codex app-server` на stdio (общий демон codex пользователя не используется), один на
отпечаток, много тредов; типы протокола не генерируются — ручное чтение используемых полей и контрактные тесты (§2.1
п. 8); граница хода — только `turn/completed`; 429 без `Retry-After` повторяет политика; следующий промпт ядро шлёт после
конца хода (два `turn/start` слились бы в steer); хуки — на тред с `bypass_hook_trust`, окружение у них процессное.
Версия — линия релизов в `mise.toml` и контрактные тесты при обновлении (app-server `[experimental]`, частые релизы).
Песочница: по умолчанию `danger-full-access` (§2.1 п. 1); `workspace-write` открыт **[НП]**, опыт — после выпуска.
Код: `crates/aae-codex/`. Описание — `docs/design/contracts.md` (4.5, 4.6 «codex»), `docs/design/experiments.md`
(опыты 2, 4–6), `docs/harness-updates.md`.
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §7.7 — сжимать: да; 8 строк (524–531)

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| показывает прогоны, живой поток, ответы на запросы разрешений | frontends.md HTTP API (live view, surface) | frontends.md «Humans and runs» |
| ссылка на сессию в состоянии прогона; открыть разговор средствами харнесса, aae не участвует | openapi.json:2089 (`session`) | frontends.md «Humans and runs» |
| подключение человека — следующий цикл [Р]; нет операций, состояний, заглушек | — | frontends.md «Humans and runs»; contracts.md 4.1 |

```text
Реализовано: `aae` показывает прогоны, отдаёт живой поток прогона и принимает ответы на запросы разрешений; у попытки
в состоянии прогона видна ссылка на сессию харнесса, по ней человек после прогона открывает разговор средствами самого
харнесса, без участия `aae`. Подключение человека к живой сессии и передача управления — следующий цикл разработки
**[Р]**; в первом выпуске нет ни операций, ни состояний, ни заглушек. Код: `crates/aae-front/src/api/`,
`crates/aae-core/src/run/row.rs`. Описание — `docs/api/frontends.md` («Humans and runs»).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

### §16 — сжимать: да; 18 строк (1026–1043)

| Требование | Где в docs/ | Дописано мной |
|---|---|---|
| опыты 1–7 и зачем | experiments.md разделы | experiments.md, таблица опытов со статусом |
| опыт 6: песочница отложена, до того danger-full-access | experiments.md:246 (не проверено) | таблица, строка 6 |
| опыт 7 opencode — следующий выпуск | — | таблица, строка 7 |
| опыты вне бюджета, ≤ 5 000 строк | experiments.md вводная; scripts/budget.sh | — |
| «сначала опыты, потом ядро» | — | experiments.md, фраза перед таблицей |

```text
Сделано: опыты 1–5 и 6 (кроме песочницы codex); результаты, замеры и следствия для адаптеров — `docs/design/experiments.md`
(таблица опытов в начале), код записанных опытов удалён (§2.1 п. 17), логи — в `experiments/`. Не проверен вход codex
по подписке в отдельном `CODEX_HOME` (§6.4). Отложены: песочница codex из опыта 6 — после выпуска, на хосте с userns или
macOS (§2.1 п. 1); опыт 7 (opencode, вопросы §7.6) — к следующему выпуску. Новый опыт — отдельный Cargo workspace в
`experiments/`, вне бюджетов, в сумме ≤ 5 000 строк (`scripts/budget.sh`).
Сжато: аудит-2, оригинал — git show ef08773:docs/init/AAE-SPEC.md
```

## Изменённые мной файлы docs/

- `docs/design/contracts.md`: 4.1 (абзац «нет steer…»), 4.2 `SessionRef`, 4.4 вводная, 4.5 (ретраи), 4.6 (абзацы
  под таблицей: общие обязанности, «claude», «codex»), 8 (два первых пункта). Разделы 3.x и 6.4 не трогал.
- `docs/design/experiments.md`: заголовок, таблица опытов, маркеры `claude-tool-heartbeat` и `codex-appserver-rss`
  (0.166.0), абзац «Types», ссылка на отпечаток.
- `docs/harness-updates.md`: вводная (проверенный диапазон вместо «pinned», линия релизов, маркер cadence), две
  ссылки «spec §7.8».
- `docs/api/frontends.md`: раздел «Humans and runs».

## Ссылки на секции группы

Заменил в docs/: contracts.md «§7.1» ×2, «§7.2» ×2, «§7.3», «§7.5» → внутренние ссылки 4.x/8; experiments.md
«spec §16» (заголовок), «spec §7.3» ×2; harness-updates.md «spec §7.8» ×2 (не моя группа, но в целевом файле).
Осталось (не трогал):
- `docs/api/openapi.json:2089` «spec §7.7» — генерируется из `crates/aae-front/src/api/replies.rs:63`;
- `docs/postmortems/0001-...md:48` «§7 failure kinds» — исторический документ;
- `docs/design/experiments.md:20` «spec §7.6» — §7.6 не сжимается;
- crates: `crates/aae-core/src/driver/failure.rs:1,5` (§7.1), `crates/aae-core/src/driver.rs:2` (§7.2),
  `crates/aae-core/src/run/registry.rs:1` (§7.3), `crates/aae-testkit/tests/instances.rs:1` (§7.3),
  `crates/aae-codex/src/stream.rs:3` (§7.5), `crates/aae-codex/tests/contract.rs:104` (§7.2),
  `crates/aae-front/src/api/replies.rs:63` (§7.7), `crates/aae-core/src/run/row.rs:135` (§7.7);
  scripts: `scripts/budget.sh:49,54` (§16).

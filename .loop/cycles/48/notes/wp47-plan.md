# WP-47 · обобщённый `[[service]]` и мост stdio→HTTP — план (expert, цикл 48)

Источники: спека §2.1 п. 4–5 [Р], §11.4, §7.3 (реестр), §6.4 (doctor о stdio хоста — остаётся), §14 (бюджеты, «без
кода на вырост»), шпаргалка `aae-context/04` §2.2–2.3 (эпохи MCP, рестарт). Состояние кода — `3ec7cbe`.
Бюджеты до работы: ядро 7962/12000, запас 863/4300, фронтенды 1885/4000. План добавляет ≈130 строк в ядро,
≈170 в запас (`crates/aae/src/bridge.rs`), ≈6 во фронтенды; из `doctor.rs` уходит ≈35.

## Главная мысль

Внутри ядра уже есть всё, что нужно сервису: запись реестра, счётчик потребителей, простой, рестарт на том же порту,
подметание по маркеру. Не хватает трёх вещей, и только их и делаем:

1. **Форма.** Сервис объявляется своей сущностью `[[service]]`, а не парой `command` + `url` внутри `mcp.<name>`;
   MCP у него по желанию; здоровье — порт, HTTP или команда.
2. **Мост.** stdio-MCP с областью не «сессия» становится тем же сервисом: команда — `aae mcp-bridge -- <command>`.
   Мост — подкоманда бинарника `aae` (модуль `crates/aae/src/bridge.rs`, строка бюджета «запас»), ядро знает о нём
   только «префикс команды», который кладёт бинарник (как `TrackerLink`).
3. **Адаптеры claude и codex не меняются**: до `split` сессия по-прежнему видит либо `{url, headers}`, либо
   `{command, env}` (stdio «на сессию»).

## Разбиение

| Кусок | Что | Размер | Кто | Файлы продукта |
|---|---|---|---|---|
| **1** | `[[service]]` в ядре: форма, слои, здоровье http/command, сервис без MCP, перевод `mcp.<name>` | M (~120–150k) | coder | `aae-core/src/config/{service.rs (новый), files.rs, load.rs, assemble.rs, mod.rs}`, `aae-core/src/run/{services.rs, service_proc.rs (новый), machine.rs, exec.rs}` |
| **2а** | Модуль моста, сам по себе | S (~60k) | subcoder, параллельно с куском 1 | `aae/src/{bridge.rs (новый), main.rs}`, `aae-front/src/cli/mod.rs` (вариант команды), `aae/Cargo.toml` |
| **2б** | Автообёртка stdio-MCP в ядре, c14 с stdio за мостом, doctor, документы | S (~60k) | coder, после 1 и 2а | `aae-core/src/run/services.rs`, `aae-core/src/config/mod.rs`, `aae/src/{harness.rs, doctor.rs, daemon.rs}` |

Куски 1 и 2а не пересекаются ни одним файлом: 2а проверяется на нынешней форме сервиса или вообще без демона.
2б зависит от обоих. Если в цикле 48 влезают только 1 + 2а — это нормальный итог; 2б — первым делом цикла 49.
Минимум таблицы ревьюера (5) поднимется после 2б; после куска 1 поднимается содержание строки «реестр экземпляров и
сервисов» (обобщённый сервис, здоровье), что надо отразить в `docs/READINESS.md`.

---

## Кусок 1 · `[[service]]` в ядре

### Форма конфига

`[[service]]` — массив таблиц верхнего уровня в трёх файлах: `config.toml` оркестратора, файл проекта (`aae.toml`),
`plugin.toml`. Неизвестные ключи — ошибка, как везде.

| ключ | тип | умолчание | смысл |
|---|---|---|---|
| `name` | строка | обязателен | имя сервиса; под ним же сессии получают его MCP. Проверка — `Loader::name` (не пусто, без `.`); два одинаковых имени в одном файле — ошибка |
| `command` | массив строк | обязателен, не пуст | argv; `${AAE_SERVICE_PORT}` в любом аргументе заменяется портом, `${AAE_PLUGIN_DIR}` (в `plugin.toml`) — каталогом плагина при загрузке |
| `env` | таблица строк | пусто | добавка к чистому окружению (как сейчас у `mcp.<name>.env`) |
| `scope` | `"orchestrator"` \| `"project"` | `"orchestrator"` | один процесс на оркестратор или на проект. `"session"` отвергает serde со своим текстом — закрывает аудит Г «неверный текст ошибки» |
| `health` | `"http"` \| `{ http = "/path" }` \| `{ command = [..] }` | нет ключа — «порт принимает соединения» | `"http"` — угаданный путь `/health`; команда: код 0 = здоров |
| `mcp` | `true` \| `{ url = "/path" }` | нет ключа — MCP не отдаётся | `true` — угаданный путь `/mcp`; сессии получают `http://127.0.0.1:<port><path>` |

Пути обязаны начинаться с `/` (проверка при загрузке). Процессу, как и сейчас: `AAE_SERVICE_PORT`,
`AAE_SERVICE_TOKEN`, `AAE_INSTANCE`.

Типы (новый `config/service.rs`, ~60 строк; `load.rs` — 385 строк, туда только вызовы):

```rust
pub(super) struct ServiceFile { name, command, env, scope: ServiceScope, health: Option<HealthFile>, mcp: Option<McpFile> }
#[serde(untagged, expecting = "...")] enum HealthFile { Guess(Word /* "http" */), Http { http: String }, Command { command: Vec<String> } }
#[serde(untagged, expecting = "...")] enum McpFile { Guess(bool), At { url: String } }

pub struct Service { pub command: Vec<String>, pub env: BTreeMap<String, String>, pub scope: ServiceScope,
                     pub health: Health, pub mcp: Option<String> }          // Clone, Debug, PartialEq, Serialize, Deserialize
pub enum Health { Port, Http(String), Command(Vec<String>) }
pub enum ServiceScope { Orchestrator, Project }                             // Default = Orchestrator
```

### Слои и потребители

Самый дешёвый путь — существующее слияние слоёв:

- `Settings` получает поле `#[serde(skip)] pub services: BTreeMap<String, Service>` (в файле такого ключа нет:
  `deny_unknown_fields` отвергнет `[settings.services]`). Загрузчик кладёт туда `[[service]]` своего файла:
  `OrchestratorFile.service` → `Config.settings` (слой 2), `ProjectFile.service` → `Project.settings` (слой 3),
  `PluginFile.service` → `Plugin.agent` (слой 5).
- `EffectiveConfig` получает `pub services: BTreeMap<String, Sourced<Service>>`; в `apply` — одна строка
  `by_key(&mut self.services, &settings.services, from, |_| true)`; в `explain` — строки `service.<name> = {...}  # слой`.
- Отсюда правило потребителей без нового кода: сервис оркестратора потребляют все сессии, сервис проекта — сессии
  проекта, сервис плагина — сессии агентов, которые плагин подключили. Старший слой заменяет запись целиком.

### Ядро: `bind_services` и процесс

`run/services.rs` — 395 строк. Сначала механический вынос: `start`, `stop`, `lock`, `Running`, `Entry` и константы
процесса → `run/service_proc.rs` (≈85 строк; правка путей в `exec.rs:39,55,58`, `mod.rs:22,75`). Отдельным коммитом.

Затем в `bind_services` (сигнатура возвращает `(config, ids: Vec<String>, ports: Vec<u16>)`):

1. `for (name, s) in std::mem::take(&mut config.services)`: определение
   `json!({ "command", "env", "health" })`, область из `s.scope`; `Self::service(tx, name, &scope, &definition)`.
   Путь `mcp` в ключ определения не входит: смена пути не перезапускает процесс.
2. Есть `mcp` → `config.mcp.insert(name, Sourced { value: Mcp { url: http://127.0.0.1:<port><path>, headers:
   Authorization }, from })`; порт идёт в `ports` (запуск ждёт только такие порты, см. `exec.rs:183`).
   Запись `mcp.<name>` с тем же именем сервис заменяет.
3. Нет `mcp` → в `config.env` две переменные от того же слоя: `AAE_SERVICE_<NAME>_URL = http://127.0.0.1:<port>` и
   `AAE_SERVICE_<NAME>_TOKEN` (`NAME` — имя заглавными, не-алфавитно-цифровое → `_`). Порт в `ports` не идёт:
   сервис без MCP может не слушать порт, а запуск не должен ждать 30 с.
4. Дальше — нынешний цикл по `config.mcp`: `${AAE_API}` и заголовки идентичности всем HTTP-записям. Ветка
   `mcp.command.take()` (сервис из `command` + `url`) удаляется.

После шага 1 `config.services` пуст: адаптер и отпечаток экземпляра его не видят.

В `service_proc.rs::start`:

- `${AAE_SERVICE_PORT}` в аргументах `command` заменяется портом;
- готовность: вместо `wait_up(&ports)` — цикл `healthy(&health, port, token, &env)` с шагом 100 мс до `READY_WAIT`:
  `Port` — `TcpStream::connect`; `Http(path)` — `GET` руками по `TcpStream` (`Host`, `Authorization: Bearer`,
  `Connection: close`), здоров при статусе 2xx — новый крейт не нужен; `Command(cmd)` — `process::run` с окружением
  сервиса и таймаутом 5 с, здоров при `Exit::Code(0)`;
- остальное без изменений: не готов за `READY_WAIT` → стоп → `ServiceFailed`.

`Mcp::well_formed`: ровно одно из `command` (не пуст) и `url`. Оба сразу — ошибка с подсказкой:
``mcp `x`: `command` with `url` is a service: declare `[[service]]` with `mcp` ``. Совместимости со старой формой
нет: пользователей нет, спека говорит «переходят».

### Тесты (все без модели)

`crates/aae-core/src/tests/config.rs`:
- `[[service]]` в трёх файлах грузится; `explain` показывает `service.<name>` с верным слоем (оркестратор, проект,
  плагин — только у агента, подключившего плагин); проект заменяет одноимённый сервис оркестратора;
- угадывание: `mcp = true` → `/mcp`, `health = "http"` → `/health`, явные пути их заменяют;
- ошибки: `scope = "session"`, пустой `command`, путь без `/`, `health` с `http` и `command` разом, два сервиса с
  одним именем в файле, `mcp.<name>` с `command` + `url` (текст называет `[[service]]`); существующий тест
  `config.rs:267` переписать под новый текст.

`crates/aae-testkit/tests/services.rs` (три нынешних теста переводятся на `[[service]]` в `shop/aae.toml` или
`config.toml`; утверждения прежние) плюс:
- `a_service_is_ready_only_once_its_health_command_passes`: `health = { command = ["test", "-e", <флаг>] }`;
  сессия уже держит URL и `call(..) == 200`, а `service.ready` ещё 0; создать флаг → `until_count("service.ready", 1)`;
- то же для `health = "http"`: в `aae-testkit/src/mcp.rs` добавить `GET /health` → 200; `{ http = "/nope" }` и
  `{ command = ["false"] }` → `service.failed` после `READY_WAIT`. Отрицательные случаи ждать только через
  `tokio::time::sleep` (часы на паузе пробегают 30 с сами), без `std::thread::sleep` из `until_count`, иначе тест
  идёт 30 с по стене;
- `a_service_without_mcp_reaches_the_session_as_its_url_and_token`: у сессии нет MCP-записи, в `env` обе переменные,
  `service.starting` один на две сессии, запуск не ждёт порта (сервис `sleep 60` с `health = { command = ["true"] }`,
  состояние `running` сразу);
- область: сервис с `scope = "project"` в двух проектах — два процесса; `orchestrator` — один.

Мутации для отчёта (каждая должна краснить тест): `healthy` всегда `true`; убрать подстановку порта в `command`;
вернуть порт сервиса без MCP в `ports`; убрать `by_key` для `services`.

Перед коммитом новые тесты `services.rs` — 30+ параллельных запусков (три флейка подряд в прошлых циклах).

### Перевод существующего

- `crates/aae-acceptance/src/scenarios/c14_shared_mcp.rs:48` — `[[service]] name = "probe"`, `mcp = true`.
- `crates/aae-claude/src/tests.rs:82-94` — не трогать в этом куске (stdio с областью проекта; меняется в 2б).
- `docs/design/config.md` раздел «MCP servers and services»: таблица `[[service]]`, три вида здоровья, сервис без MCP
  и его переменные, перенос старой формы; строка 34 таблицы настроек. `docs/guide/plugin.md:70-74` — пример на
  `[[service]]`. `docs/design/contracts.md` §8 (ключ определения, здоровье, `EffectiveConfig.services` в 4.2).
- `docs/READINESS.md`: строки «Instance and service registry» и «Shared MCP» — что сделано, что ждёт моста.
- `.loop/PLAN.md`: WP-47 → split на 47a (кусок 1), 47b (2а + 2б).

### Готово, когда

- `cargo nextest run -p aae-core -p aae-testkit` зелёный, `mise run acceptance-ci` — c14 зелёный на новой форме;
- `grep -rn 'AAE_SERVICE_PORT}/mcp' crates docs --include=*.rs --include=*.md` вне `docs/init` пусто (старой формы нет);
- `scripts/budget.sh`, `scripts/core-boundary.sh`, clippy `-D warnings` чисто; ни один файл не больше 400 строк
  (`services.rs`, `load.rs`, `machine.rs` — проверить явно);
- четыре мутации убиты и откачены.

---

## Кусок 2а · модуль моста (`aae mcp-bridge -- <command>`)

### Процессная модель

Мост — обычный процесс сервиса: демон запускает `<aae> mcp-bridge -- <command…>` тем же путём, что любой
`[[service]]`. Мост порождает stdio-сервер ребёнком (stdin/stdout — каналы, stderr наследуется, чтобы хвост попал в
`service.failed`; `kill_on_drop`), окружение наследуется целиком — маркер `AAE_INSTANCE` доходит до ребёнка, и
подметание реестра убирает обоих. Ребёнок умер или закрыл stdout → мост выходит с кодом ≠ 0 → реестр перезапускает
на том же порту (уже есть). Мост умер → ребёнок получает EOF на stdin.

### Протокол

Сторона stdio (один клиент — сам мост): при старте `initialize` (`protocolVersion` — первая из списка
`initialize`-эпохи, `capabilities: {}`, `clientInfo: aae-mcp-bridge`), ждать ответ ≤ 25 с, сохранить `result`,
послать `notifications/initialized`. **Порт привязывается только после этого**: «порт принимает соединения» значит
«stdio-сервер готов».

Сторона HTTP — как `aae-front/src/api/mcp.rs`, обе эпохи [Р], без `Mcp-Session-Id` (рестарт моста не оставляет
клиентов с мёртвой сессией), `GET /mcp` → 405:

| запрос | ответ моста |
|---|---|
| нет `Authorization: Bearer $AAE_SERVICE_TOKEN` | 401 |
| не один JSON-объект | 400, `-32700` |
| без `id` или без `method` (уведомление, ответ клиента) | 202, в stdio не идёт |
| `initialize` (не новая эпоха) | из сохранённого `result`: `capabilities`, `serverInfo`, `instructions`; `protocolVersion` — запрошенная, если она в списке, иначе первая (правило `api/mcp.rs`) |
| `server/discover` с `MCP-Protocol-Version: 2026-07-28` | `supportedVersions: ["2026-07-28"]`, `capabilities` из сохранённого |
| любой другой запрос | в stdio: `id` заменить своим счётчиком, в `params._meta` добавить `aae/project`, `aae/rig`, `aae/agent`, `aae/run` из заголовков `X-Aae-*`; дождаться ответа с этим `id`, вернуть клиенту с его `id`; в новой эпохе добавить `resultType: "complete"`, если его нет |

Состояние: `Mutex<HashMap<u64, oneshot::Sender<Value>>>` и писатель в stdin под мьютексом; одна задача читает stdout
построчно и раздаёт ответы по `id`. Запросы и уведомления сервера к клиенту отбрасываются (мост объявил пустые
возможности клиента) — комментарий `shortcut:` в коде. Константы эпох взять из `aae-front` (сделать `pub`), не
дублировать. `axum` в `crates/aae` — `cargo add axum -p aae` без версии (в lock он уже есть), фичи как у `aae-front`.

Точка входа: вариант `Command::McpBridge { #[arg(last = true, required = true)] command: Vec<String> }` в
`aae-front/src/cli/mod.rs` (скрытый из `--help`, описан в `config.md`), в `main.rs` — `return bridge::run(command)`.
`run` читает `AAE_SERVICE_PORT` и `AAE_SERVICE_TOKEN` из окружения; без них — ошибка в stderr и код 1.

### Тесты

Тестовый stdio-сервер — в `aae-testkit/src/mcp.rs`: `serve_stdio(log)` (строки JSON-RPC: `initialize`,
`tools/list`, `tools/call` инструмента `whoami_stdio`, который возвращает `_meta` вызова; в лог — pid, метод, `_meta`)
и режим `mcp-probe --stdio <log>` в `aae-testkit/src/bin/mcp-probe.rs`.

`crates/aae/tests/bridge.rs` (настоящий бинарник `aae`, без демона):
- порт принимает соединения только после `initialize` ребёнка; без токена — 401;
- `initialize` с 2025-06-18 → та же версия и `serverInfo` ребёнка; `server/discover` с заголовком новой эпохи →
  `supportedVersions`; `tools/call` в новой эпохе несёт `resultType`;
- два клиента одновременно шлют `tools/call` с одним и тем же `id: 1` и разными `X-Aae-Run` → каждый получает свой
  ответ со своим прогоном; в логе один pid и один `initialize`;
- ребёнок убит → мост выходит с кодом ≠ 0.

Мутации: не менять `id` (второй тест красный); привязать порт до `initialize`; не проверять токен.

### Готово, когда

`cargo nextest run -p aae bridge` зелёный (и 30 параллельных запусков), три мутации убиты, `budget.sh` — запас
≈1030/4300, `bridge.rs` ≤ 400 строк, clippy чисто. Ядро этим куском не тронуто.

---

## Кусок 2б · автообёртка, c14, doctor

### Ядро

- `Config` получает `pub bridge: Vec<String>` — «префикс команды, которая отдаёт stdio-MCP по HTTP на
  `AAE_SERVICE_PORT`; пусто — моста нет» (`config::load` оставляет пустым, как `TrackerLink`). Слова `br` в ядре нет:
  `core-boundary.sh` ищет его целым словом, `bridge` проходит.
- `bind_services`, перед циклом заголовков: запись `config.mcp` с `command`, без `url`, с областью не `session`, при
  непустом `daemon.bridge` → тот же путь, что у `[[service]]` с `mcp = "/mcp"`: команда `bridge ++ command`, `env`
  записи, область `project` либо `orchestrator`, здоровье `Port`. Запись превращается в `{url, headers}`. ≈12 строк.
- Умолчание области stdio-MCP меняется по [Р] §11.4 («по умолчанию один на область»): без `scope` — `orchestrator`,
  то есть общий за мостом; «на сессию» — только явный `scope = "session"`.

### Бинарник

- `aae/src/harness.rs::load`: `config.bridge = [<путь бинарника>, "mcp-bridge", "--"]`. Путь — взятый при старте
  процесса в `main.rs` (тот же, что у рестарта), не `current_exe()` в момент reload: после замены файла Linux отдаёт
  `… (deleted)` (урок cbd0a6e).
- `aae/src/doctor.rs`: `per_session`, `warn_per_session`, проверка «mcp per session» и их вызовы в `daemon.rs:61,180`
  удаляются вместе с тестом `tests/doctor.rs:169-180` — предупреждать больше не о чем. Предупреждение о stdio-MCP
  хоста (§6.4) остаётся; в его тексте добавить «объяви сервер в конфиге aae — он станет общим».

### Тесты

- `aae-testkit/tests/common/mod.rs::core_on`: `config.bridge = vec!["env".into()]` — `env <command>` прозрачно
  запускает команду, так что «мостом» в тестах ядра служит сам HTTP-зонд. Тест в `services.rs`: stdio-запись без
  `scope` у двух сессий → один `service.starting`, у обеих `url` + токен + заголовки, `command` нет;
  `scope = "session"` → запись дошла до драйвера как stdio, сервисов нет; `scope = "project"` в двух проектах — два.
- `aae-claude/src/tests.rs:82-94` — утверждение «no bridge: per session» заменить: адаптер получает stdio только с
  `scope = "session"` (сам адаптер не меняется).
- **c14**: рядом с `[[service]] probe` добавить `[settings.mcp.notes] command = [<probe>, "--mcp-stdio-probe", <log2>]`
  (режим в `aae-acceptance/src/main.rs` рядом с `--mcp-probe`). План провайдеров:
  `Mcp(TOOL)`, `Mcp("whoami_stdio")`, финишер, текст. Утверждения для stdio: в `log2` один pid и один `initialize`
  на 2 claude + 2 codex; у каждого прогона свой `tools/call` с `_meta["aae/run"]`; `service.starting` для `notes` —
  один, `service.failed` нет. Это настоящие `claude` и `codex` против моста — единственная проверка обеих эпох
  на живых клиентах.
- Просмотреть тесты со stdio-MCP без `scope` (`grep -rn 'command = \[' crates/*/tests crates/aae-acceptance`) — те,
  что ждут «на сессию», получают `scope = "session"`.

Мутации: пустой `bridge` в `harness.rs::load` → c14 красный (два pid); убрать условие `scope != session` → тест ядра.

### Документы

`docs/design/config.md` (stdio-MCP общий по умолчанию, когда ставить `scope = "session"`: состояние на клиента,
зависимость от рабочего каталога — процесс сервиса стартует в `/`; что мост не пересылает: запросы и уведомления
сервера, отмену), `contracts.md` §8, `docs/guide/plugin.md`, `docs/guide/install.md` (подкоманда `mcp-bridge` —
внутренняя), `docs/READINESS.md` обе строки, отчёт приёмки критерия 14. Журнал: допущения ниже.

### Готово, когда

`mise run ci` и `mise run acceptance-ci` зелёные; c14 под нагрузкой (10 параллельных `aae-acceptance --ci 14`) —
10/10; `grep -rn 'no stdio→HTTP bridge' crates docs` пусто; WP-47 в плане — `doing` до зелёного CI.

---

## Спорные места — записать в журнал как РЕШЕНИЕ

1. **[Д] «Угадывание путей» — статическое.** `mcp = true` → `/mcp`, `health = "http"` → `/health`; без ключа `health`
   — порт, без ключа `mcp` — MCP нет (так написано в комментариях примера §11.4). Зондирование путей на живом
   процессе не делаем: недетерминированно и ходит в чужой сервис без спроса. Если владелец имел в виду зондирование —
   это отдельный S.
2. **[Д] Сервис без MCP виден сессии через `AAE_SERVICE_<NAME>_URL` и `_TOKEN`.** Спека не говорит, как сессия
   находит такой сервис; без этого порт, выбранный демоном, никому не известен, и п. 5 [Р] был бы пустым.
3. **[Д] Здоровье = готовность.** Проверка решает, когда сервис `ready`; дальше живость — это живой процесс (как
   сейчас и как у экземпляров харнессов). Периодической проверки нет: нужен таймер на модельных часах, новый
   эффект и сообщение, а критерия §15 под неё нет. Сбой MCP — сбой инструмента (§11.4).
4. **[Д] Потребители сервиса плагина — сессии агентов, подключивших плагин** (слой 5), а не все сессии уровня, где
   плагин объявлен. Сервис оркестратора или проекта потребляют все сессии уровня. Exec-шаги сервисы не поднимают.
5. **[Р]-следствие, меняет поведение:** stdio-MCP без `scope` раньше стартовал на сессию с предупреждением, теперь
   общий за мостом. Сервер с состоянием на клиента или с зависимостью от рабочего каталога требует
   `scope = "session"` — сказать в `config.md` и в записи журнала.
6. **[Д] Идентичность за мостом — в `params._meta`** (`aae/project`, `aae/rig`, `aae/agent`, `aae/run`): stdio-сервер
   заголовков не видит. Сервер, не читающий `_meta`, просто не узнает, кто звонит.
7. **[НП] Новая эпоха через мост.** Мост отвечает на `server/discover` и добавляет `resultType`, а stdio-сервер
   говорит на `initialize`-эпохе. Других различий формата ответов между эпохами мы не знаем (лабораторный сервер —
   приближение, шпаргалка 04 §2.2). Проверка — c14 на настоящем `claude`. Если claude в новой эпохе споткнётся на
   ответе stdio-сервера — запасной вариант: мост отвечает на `server/discover` ошибкой `-32601`, claude сам
   откатывается на `initialize` (наблюдалось, 04 §2.2); это отступление от [Р] «обе эпохи», только с записью в журнал.
8. **Совместимости со старой формой нет**: `mcp.<name>` с `command` + `url` — ошибка конфига с подсказкой.
9. **Не делаем** (нет применения): unix-сокет для сервиса (loopback с токеном уже есть), `cwd` сервиса, подстановку
   токена в `command`, пересылку отмены и серверных уведомлений через мост, SSE-поток `GET /mcp`, пакетные
   JSON-RPC-массивы, периодическую проверку здоровья.

## Риски

- `services.rs` (395), `load.rs` (385), `machine.rs` (397) у лимита 400: вынос `service_proc.rs` — первым коммитом,
  новые типы — в `config/service.rs`, в `machine.rs:219,261,276` меняются только имена переменных.
- Часы на паузе и настоящие процессы: `until_count` тормозит модельные часы до настенных. Положительные тесты
  здоровья — через `until_count`, отрицательные (30 с `READY_WAIT`) — только через `tokio::time::sleep`.
- Сироты codex в приёмке (жел. 1 ревью 47): c14 после 2б оставляет ещё и мост с ребёнком, если демон убит `kill -9`.
  Зонды testkit выходят, когда умер родитель; stdio-зонд выходит по EOF на stdin — проверить это в тесте «ребёнок
  моста не переживает мост».
- Запись `instances.process` меняет форму (`health` вместо `url`). Миграция не нужна: старт демона останавливает
  все записанные сервисы (`recover.rs`), новые создаются в новой форме.

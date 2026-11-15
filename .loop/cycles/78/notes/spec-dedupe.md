# WP-65: спека без дублей docs/

Строк в `docs/init/AAE-SPEC.md` (`wc -l`): до — 1177, после — 1122.

Правило, по которому шёл: убирал только фразы без метки [Р]/[Д]/[НП] (предложение или пункт, к которому метка не
относится), смысл которых полностью есть в `docs/`. Помеченные решения и допущения не трогал, даже если docs их
повторяют. §1–§3, §8.2, §9.1, §10.3, §14, §15 (с «Демо»), §18 не трогал. Следов истории (циклы, WP-, sha, «реализовано»)
в спеке не нашлось: sha в §14 — внутри маркера `MEASURE`, это замер, а не история.

## Убранное

| § | что убрано | где это в docs/ |
|---|---|---|
| 4 | «Фронтенды не содержат доменной логики: каждый только переводит свой протокол…» | `docs/api/frontends.md`, вступление |
| 4 | «Адаптер харнесса не принимает решений о повторах, парковке и исходе…» | `docs/design/adapters.md`, вступление и «What the core expects of a driver» (последний пункт) |
| 5.1 | «Resync обязателен при старте и раз в несколько минут; неоднозначное расхождение — `resync.diverged` и ноль эффектов» | `docs/design/event-model.md` «Resync»; `docs/design/invariants.md`, инвариант 20 |
| 5.2 | «SQLite в режиме WAL, статически слинкованная» | `docs/design/event-model.md` «State» |
| 5.2 | «Единственный писатель — демон; CLI, плагины и агенты — только через API» | `docs/design/event-model.md` «State» |
| 5.2 | «В состоянии только рантайм; определения в файлах; прогон хранит снимок эффективного конфига» | `docs/design/event-model.md` «State» |
| 5.2 | «Файлов-статусов, PID- и lock-файлов нет; живость — pid + время старта; второй демон не стартует» | `docs/design/event-model.md` «State» |
| 5.3 | «Журнал — канал наблюдения, истина — состояние; чтение с `seq`, вытесненный `seq` — ошибка „разрыв“» | `docs/design/event-model.md` «Journal» |
| 5.4 | «Наблюдаемые события читают, но не влияют; решающие ждут exec-хука» | `docs/design/contracts.md` 1.3 |
| 5.4 | Что делает ядро без решения плагина (5 решающих событий) | `docs/design/contracts.md` 1.3 (таблица, колонка «No plugin decision»), 3.3 |
| 5.4 | «Каждый отказ ядра действовать пишет событие с причиной: молчаливых ожиданий нет» | `docs/design/contracts.md` 1.3 (абзац после таблиц) |
| 5.4 | «Схема события, нагрузки и решения хуков — контракт, без смены версии не меняются» | `docs/design/contracts.md`, вступление и 1.1 «Version rules» |
| 5.5 | «Двадцать инвариантов — знание предшественника; номера стабильны, ссылки „invariant N“» | `docs/design/invariants.md`, вступление |
| 5.5 | «Все, кроме 19-го, — механизмы ядра, не вырезаются ради бюджета; пороги — конфиг, реакция — политика» | `docs/design/invariants.md`, вступление |
| 5.5 | «Проверка — детерминированный хаос-стенд» | `docs/design/invariants.md`, абзац после таблицы |
| 5.6 | «Плановый рестарт — с дренажом» | `docs/design/invariants.md` «Recovery» |
| 5.6 | «Доставка промпта — не менее одного раза с идемпотентным эффектом; адаптер отвечает по транскрипту» | `docs/design/invariants.md` «Recovery»; `docs/design/contracts.md` 6.4 |
| 5.7 | «Единственный механизм „не запускать пока“…; ждёт с видимой причиной; светофор и удержание без подсистем» | `docs/design/contracts.md` 7 |
| 5.7 | «Закрывают политика, оператор и ядро; закрытые оператором автоматика не снимает» | `docs/design/contracts.md` 7 |
| 6.5 | «Эффективный конфиг собирает ядро при подготовке; адаптер — в флаги, параметры, файлы в каталоге состояния» | `docs/design/config.md` «Effective config and "explain config"» (конец) |
| 6.5 | «Окружение сессии собирается из слоёв, а не наследуется от демона» | `docs/design/config.md` «Effective config…»; `docs/design/adapters.md` «What the core expects…» |
| 6.5 | «Невалидный конфиг не применяется, идущие прогоны работают со снимком» | `docs/design/config.md` «Loading» |
| 6.5 | «`aae doctor` проверяет каждый бинарник, его версию и логин» | `docs/guide/install.md` «Check the setup» |
| 7.1 | «Ожидание человека и фоновая работа вне хода — состояния, а не сбои» | `docs/design/contracts.md` 4.4 |
| 7.3 | «Что „процессное“, решает адаптер: claude — процесс на сессию, codex — app-server на отпечаток» | `docs/design/contracts.md` 8 и 4.6 |
| 7.4 | «Транспорт `claude -p`, stream-json, id сессии; полный набор настроек с моделью при каждом resume» | `docs/design/contracts.md` 4.6 (claude) |
| 7.4 | «Время сброса лимита — только при подписке; при API-ключе — `rate_limited` без времени» | `docs/design/contracts.md` 4.6 (claude) |
| 7.5 | «Транспорт: свой `codex app-server` на stdio…; демон codex пользователя не используется» | `docs/design/contracts.md` 4.6 (codex и таблица) |
| 7.5 | «app-server `[experimental]`: линия релизов, обновление принимается контрактными тестами» | `docs/design/contracts.md` 4.6 (codex); `docs/guide/install.md`; `docs/harness-updates.md` |
| 7.7 | «aae показывает прогоны, живой поток, принимает ответы; у попытки ссылка на сессию» | `docs/api/frontends.md` «Humans and runs» |
| 7.8 | «15 сценариев против настоящего бинарника с поддельным провайдером, без затрат на LLM» | `docs/harness-updates.md` «How the suite is built» |
| 7.8 | «Набор в CI и при каждом обновлении; обновление принимается, когда проходит» | `docs/harness-updates.md` «How the suite is built»; `docs/design/adapters.md` «Contract suite» |
| 7.8 | «У claude на поддельном провайдере нет времени сброса: сценарий ждёт `rate_limited` без времени» | `docs/harness-updates.md` «How the suite is built» (Known limit) |
| 9.2 | «Сценарии без модели — подмножество той же spec, CI на каждый коммит» | `docs/guide/development.md` «Acceptance scenarios» |
| 9.2 | «Результат — один отчёт с датой, версией aae и версиями харнессов» | `docs/guide/development.md` «Acceptance scenarios» |
| 9.3 | Цепочка состояний `queued → … → cancelled` | `docs/design/contracts.md` 6.1 |
| 9.3 | «Сбой и рестарт → `interrupted`; ожидание ворот → `parked`; замечание: `finishing` → `running`» | `docs/design/contracts.md` 6.1, 6.2 |
| 9.3 | Классы провала `agent`, `check`, `hollow_done`, `infrastructure` | `docs/design/contracts.md` 6.1 |
| 9.5 | «Оператор включает и выключает триггер и может сработать его вручную» | `docs/design/config.md` «How runs start and what a firing guarantees» |
| 9.5 | «Срабатывание идемпотентно по (триггер, `seq`), в транзакции события; `kill -9` не теряет и не удваивает» | `docs/design/config.md` «How runs start…» |
| 9.5 | «Язык `where`/`inputs`, таймауты хуков, срок жизни outbox — контракт, без слома не меняются» | `docs/design/contracts.md`, вступление; `docs/design/config.md` «How runs start…» (последний пункт) |
| 10.2 | «Путь к БД задаётся явно в каждой сессии и вызове» | `docs/design/tracker-br.md` «One database per project» |
| 10.2 | «У каждой попытки свой actor; ничего служебного в трекер не пишется» | `docs/design/tracker-br.md` «One database per project» |
| 10.2 | «Версия br закреплена точно, у адаптера свой набор контрактных тестов» | `docs/design/tracker-br.md` «Versions» |
| 11.1 | «Каталог с `plugin.toml` (имя, версия, параметры, вклад, хуки, сервисы) и `skills/`, `hooks/`, `bin/`» | `docs/guide/plugin.md`, вступление, «The manifest», «The agent side» |
| 11.2 | «Вклад в „процессную“ часть даёт агенту отдельный экземпляр» | `docs/design/contracts.md` 8 |
| 11.4 | «Сервис объявляется в конфиге или плагине и живёт в том же реестре, что экземпляры» | `docs/design/config.md` «MCP servers and services»; `docs/design/contracts.md` 8 |
| 11.5 | «Граница: механизмы в ядре…; выбор реакции и пороги — в плагине; без `default-policy` — умолчания ядра» | `docs/design/config.md` «Where the bundled plugins end and the core begins» |
| 12 | HTTP API: unix-сокет, TCP только loopback с токеном, SSE с `seq`, OpenAPI из кода с проверкой, ключ идемпотентности | `docs/api/frontends.md` «HTTP API» |
| 12 | CLI: тонкий клиент HTTP API, команда на каждую операцию с тестом, JSON-вывод везде | `docs/api/frontends.md` «CLI» |
| 13 | «`aae doctor` (харнессы, br, права, состояние) работает без демона» | `docs/guide/install.md` «Check the setup»; `docs/api/frontends.md` «CLI» |

Итого: 51 фраза. Вместо них — указатели «Подробно: …» в тех же секциях.

Оставлено, хотя похоже на дубль (в docs не полностью): §9.2 «самая дешёвая модель» (docs: «a cheap one»); §11.4
«unix-сокет либо loopback с токеном» (в docs про сервисы — только loopback); §11.3 «хук без состояния, хранилища для
плагинов в ядре нет»; §7.4 «субагенты claude по умолчанию фоновые»; §12 абзац об объяснении конфига (в docs нет
«шаг называется» и поля `agent`); §9.5 «пак не знает имён ригов».

## Расхождения

Фразы спеки не тронуты; поправить надо docs (видимо, пакетами четвёртого цикла по §8.2).

- §8.2 [Р]/[Д] против `docs/design/config.md` «Packs and plugins from git: `aae.lock` and `aae fetch`»: спека — lock-файла
  и отдельной команды закрепления нет, закрепление в конфиге, кеш один на пользователя в XDG cache, «последняя ревизия»
  выясняется при загрузке конфига и пак скачивает `aae`; docs — `aae.lock` рядом с конфигом, `aae fetch [--update]`,
  кеш `.aae/git/<commit>/` рядом с файлом, загрузка конфига git не запускает, без пина — ошибка конфига.
- §11.6 [Д] против того же раздела docs: спека — плагин из git обязан быть закреплён sha или тегом, незакреплённый —
  ошибка конфига; docs — `rev` может быть веткой, закрепление делает `aae.lock`.
- §12 (CLI без демона) против `docs/api/frontends.md` «CLI»: спека — «очистка кеша паков»; docs — `aae fetch`, команды
  очистки кеша нет.
- Не расхождение, но след истории в docs: `docs/design/contracts.md` 4.6, последний абзац — «(WP-28, WP-30)».

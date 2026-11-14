# WP-S2 — постановка, второй проход: суть и указатель во всех секциях

Файл: `docs/init/AAE-SPEC.md`. Коммиты (только этот путь, не запушено): `183f172` (§5), `815d09f` (§6, §7,
сокращения моделей), `f22ded5` (§8, §9), `61a16be` (§10–§13), `a665727` (§5–§7, второй проход), `a0713c0` (§8–§13,
второй проход). Код и `docs/design`, `docs/api`, `docs/guide` не трогал.

## Объём

Было 1303 строки, стало 1091. В ориентир 750–850 не попал, и добрать до него сокращением §4–§13 нельзя:

- 521 строка — секции, которые по заданию не трогаются по существу: преамбула (28), §1 и §1.1 (54), §2 (89),
  §3 (33), §14 (115), §15 с «Демо» (79), §16–§18 (72), §19–§20 (51).
- 570 строк — §4–§13 (было 782). Из них около 240 — заголовки, пустые строки и строки «Подробно: …» сорока
  подсекций; около 32 строк держит список «нет в docs/» (ниже); остальное — формулировки решений [Р] и [Д], по
  одной-две строки на решение.

Чтобы выйти на 850, надо резать §2 (таблица решений дублирует секции по построению), §14 или §15 — это решение
владельца, не моё.

## Таблица секций

Секции, которых в таблице нет (§1–§4, §5.2, §6.1, §7 ввод, §7.5 по объёму, §7.6, §11.6, §14–§20), не менялись.

| § | Было | Стало | Куда указывает |
|---|---|---|---|
| 5.1 | 18 | 9 | `design/event-model.md` |
| 5.3 | 12 | 8 | event-model «Journal»; contracts 1.1 |
| 5.4 | 31 | 13 | contracts 1 (1.3 — типы и нагрузки), 3 |
| 5.5 | 23 | 12 | `design/invariants.md` |
| 5.6 | 27 | 10 | invariants «Recovery», «Limits»; contracts 3.4, 6.4 |
| 5.7 | 20 | 16 | contracts 7 |
| 6.2 | 33 | 30 | config «Settings (any layer)», «Harness, provider and model» |
| 6.3 | 35 | 19 | config «Effective config and "explain config"»; experiments «Config and credential delivery» |
| 6.4 | 15 | 9 | config «Host inheritance» |
| 6.5 | 14 | 11 | config «Loading»; guide/install |
| 7.1 | 19 | 9 | contracts 4 (4.1–4.5) |
| 7.2 | 15 | 9 | adapters «Adding a harness», «What the core expects of a driver»; contracts 4.5, 4.6 |
| 7.3 | 15 | 10 | contracts 8, 4.6; experiments «codex app-server» |
| 7.4 | 11 | 10 | contracts 4.6; experiments «claude protocol», «claude processes and background work» |
| 7.5 | 16 | 16 | contracts 4.6; experiments «codex app-server»; harness-updates (пять решений [Р], текст короче) |
| 7.7 | 9 | 7 | api/frontends «Humans and runs» |
| 7.8 | 16 | 11 | harness-updates «How the suite is built»; adapters «Contract suite» |
| 8.1 | 35 | 16 | config «Agent», «Agent variants» (пример TOML убран — он есть в config «Project») |
| 8.2 | 18 | 11 | config «Pack», «Names», «Packs and plugins from git» |
| 9.1 | 27 | 16 | config «Formula»; contracts 2.3 |
| 9.2 | 14 | 8 | guide/development «Acceptance scenarios» |
| 9.3 | 39 | 22 | contracts 6; api/frontends «CLI» (схема состояний — одной строкой) |
| 9.4 | 22 | 17 | contracts 6.3 |
| 9.5 | 28 | 26 | config «Trigger», «How runs start and what a firing guarantees»; contracts 2, 3 |
| 10.1–10.3 | 42 | 32 | contracts 5; `design/tracker-br.md` («Beads and runs»); experiments «br as an event source» |
| 11.1–11.5 | 77 | 65 | guide/plugin; config «Plugin», «MCP servers and services», «Bundled plugins»; contracts 3, 8, 10 |
| 12 | 26 | 24 | api/frontends, api/mcp, openapi.json (таблица заменена списком) |
| 13 | 23 | 22 | guide/install, guide/development |

Сокращения моделей: идентификаторов версий в постановке больше нет (`grep 'gpt-'` пуст). В §6.2 — правило
(раскрывается в последнюю версию семейства; таблицу держит адаптер и сверяет со списком моделей харнесса; полное имя
передаётся как есть) и одна строка семейств. В §16 (опыт 3) «имя из таблицы §6.2» заменено на «сокращение из таблицы
адаптера (§6.2)» — иначе ссылка вела бы на таблицу, которой нет.

В §11.5 ссылка «колонка „без решения плагина“ из §5.4» заменена на «умолчания ядра из §5.4»: таблица решающих событий
в §5.4 стала перечнем (полная таблица — contracts 1.3).

## Нет в `docs/` (осталось в постановке полностью)

| Требование | § | Куда дописать |
|---|---|---|
| CI проверяет границу ядра поиском по исходникам `aae-core` (имена харнессов, `max_sessions`, `BEADS_DIR`, `.beads`) | 4 | `guide/development.md`, таблица проверок (скрипт `scripts/core-boundary.sh` там не назван) |
| Фронтенды не содержат доменной логики | 4 | `api/frontends.md`, ввод (сейчас только «holds no state of its own» — слабее) |
| Адаптер сверяет таблицу сокращений со списком моделей харнесса | 6.2, 7.2 | `design/adapters.md` «Adding a harness»; `design/config.md` «Harness, provider and model» |
| Харнесс не принимает имя из таблицы — находка для владельца; молча подменять имя нельзя | 6.2 | там же |
| Несовместимые `harness` и `provider` — ошибка конфига (в `docs/` правило другое, см. противоречие 1) | 6.2 | `design/config.md` «Harness, provider and model» после решения владельца |
| Риг прогона берётся из входа, из триггера **или из метки бида** | 9.3 | `design/contracts.md` 6.1 (там риг только называется при создании или триггером) |
| Принципы новой модели: агент не знает о задаче; в паке нет ни слова о задаче; «что» приходит только вводом пользователя | 8.1, 8.2, 9.1 | `design/config.md` «Agent», «Pack», «Formula» или `guide/first-formula.md` |
| Мост stdio→HTTP не является частью ядра; общий сервис слушает unix-сокет либо loopback с токеном | 11.4 | `design/config.md` «MCP servers and services» (описан только loopback с токеном; где живёт мост — не сказано) |
| Репозиторий публичный: никаких секретов и абсолютных путей машины владельца | 13 | `guide/development.md` |
| Вопросы спайка opencode (семь пунктов) | 7.6 | не дописывать: это задача следующего выпуска, после опыта — `design/experiments.md` |

Вместе эти требования держат в постановке около 32 строк (из них 15 — §7.6).

## Противоречия постановки и `docs/`

1. **Несовместимые харнесс и провайдер.** Постановка §6.2 [Р]: «Не задано ни то, ни другое, либо провайдер не
   поддержан заданным харнессом — ошибка конфига». `docs/design/config.md`, «Harness, provider and model»: «both are
   set and the harness does not work with the provider: the stronger layer's value stands and the other follows from
   it (an agent's `harness = "claude"` under a variant's `provider = "openai"` runs on codex); set in the same layer,
   it is an error». По преамбуле права постановка. Но правило документа — то, что позволяет демо 2 отличаться только
   вариантом с `provider = "openai"`, если агент пака назвал `harness`. Нужно слово владельца; текст постановки не
   менял.
2. **Ключ ворот модели.** `docs/design/contracts.md`, раздел 7, противоречит сам себе: «The model key is the run's
   config's … known before the first attempt» и четырьмя пунктами ниже «The model key is known once a failure
   reported `provider_model`; until then a run is admitted on its other keys». Права постановка (решение 74, §5.7):
   второй абзац — остаток прежней модели.
3. **Число слоёв.** Постановка §6.3 — 11 слоёв (умолчание модели харнесса — отдельный слой), `config.md` — 10
   (умолчание модели слито со слоем оркестратора). Порядок один и тот же, противоречия по существу нет; в постановке
   список теперь без номеров. В самом `config.md` остался устаревший комментарий в примере оркестратора:
   `[overrides.implementer]  # layer 7` (по его же списку это слой 10).
4. **Риг из метки бида.** Постановка §9.3 [Д]: «Риг берётся из входа, из триггера или из метки бида».
   `contracts.md` 6.1: «A run belongs to a project and a rig, named when it is created (API, CLI, MCP) or by the
   trigger». Допущение автора постановки; реализация его не делает. Либо убрать «из метки бида» из постановки, либо
   реализовать — решение владельца.
5. **Хуки харнесса у codex.** `guide/plugin.md`: «aae delivers them to claude as they are and translates them for
   codex»; `design/adapters.md`: «codex takes the same form». Прежняя фраза постановки «claude и codex исполняют его
   нативно» была фактом, а не требованием, — из §11.2 убрана, осталось «в диалекте Claude [И 18]». Расхождение между
   двумя документами — поправить `plugin.md`.
6. **Вводная `config.md` «Effective config».** «An agent … picks one of the orchestrator's harnesses and,
   optionally, a model» — формулировка прежней модели (по новой харнесс выводится из провайдера и меняется слоями).
   Права постановка §6.2.

## Что убрано из постановки как повтор `docs/`

Перечень наблюдаемых событий и таблица решающих (contracts 1.3); перечень двадцати инвариантов по одному
(invariants); пересчёт дедлайнов после рестарта и детали дренажа (invariants «Recovery»); перечень операций драйвера и
видов сбоя (contracts 4.1, 4.4); перечень 15 контрактных сценариев (harness-updates); пример TOML варианта агента
(config «Project»); схема состояний прогона рисунком и описания `queued`/`parked` (contracts 6.1); хвост вывода и
группа процессов exec-шага (config «Formula»); детали actor и слежения за БД br (tracker-br); таблица плагинов в
комплекте с перечнем реакций (contracts 10, config «Bundled plugins»); перезапуск сервиса (config «MCP servers and
services»).

## Самопроверка

- `grep -nE 'Реализовано|Сделано|Сжато|цикл(а|е|ы)? [0-9]|WP-[0-9]|Изменено владельцем|Уточнено|ef08773|\.loop/'` —
  пусто.
- MEASURE: набор `id=` до и после совпадает (`opencode-cli-surface`, `design-a-v0-loc`, `metropolis-glue-loc`,
  `claude-resume-time`, шаблон в §14).
- Заголовки и нумерация секций — те же 58, в том же порядке; разделителей `---` 20, как было.
- Строк длиннее 120 вне таблиц и маркеров MEASURE нет.
- Метки: [Р] 198 → 198, [Д] 58 → 57 (в §8.2 два соседних [Д] слиты в одно предложение), [НП] 4 → 4,
  [И NN] 42 → 42.
- Указатели: все файлы `docs/…` существуют; каждый названный раздел найден grep-ом по заголовку (30 разделов
  `docs/design`, `docs/api`, `docs/guide`, `docs/harness-updates.md`; номера разделов contracts 1.1, 1.3, 2.3, 3.4,
  4.4–4.6, 5, 6.3, 6.4, 7, 8, 10).
- §2: строки таблицы однострочные; текста секций не повторяют сверх одной фразы на решение. Не менял.

## Не сделано и что осталось владельцу

- Ориентир 750–850 не достигнут (1091); причины и состав — в разделе «Объём».
- Противоречия 1 и 4 требуют решения владельца.
- Сверка шла по тексту документов: соответствие `docs/` коду не проверялось.

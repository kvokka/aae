SCORE: 5.3/10
VERDICT: continue
BLOCKERS: 1

# Ревью цикла 111

Дифф `b0b7b2d..9887aae`: 5 коммитов кодера, 19 файлов, +1376/−847, в основном перенос тестов. Поверх них лежит
коммит владельца `34df23d`: решения 126–136, пересмотр 120 и пакеты WP-102…WP-108. Код в нём не менялся, и это не
правка кодера (журнал 07:53). Вне `.loop` дерево чистое, незакоммичены только `state.json` и `cycles/111/`.

`checks.md` кодера был снят на `9887aae`, а HEAD с тех пор сдвинулся на `34df23d`. Поэтому гейт я прогнал один раз
сам (`scripts/loop/checks 111`): pass, 30 s, nextest 551/551.

## Обязательно до продолжения

1. **Ранний `auth_required` claude на 403 и на «разлогиненный» `error` в `api_retry` не покрыт тестом.** Это прямое
   требование WP-97 («ранний `auth_required` в `retry()` и при 403, `oauth_org_not_allowed`,
   `cloud_credential_error`»), и contracts 4.6 его утверждает. Место в коде: `crates/aae-claude/src/stream.rs:217-218`.
   Проверил мутацией: `Some(401 | 403)` заменил на `Some(401)` и одновременно выключил проверку
   `classify::AUTH.contains`. Весь `cargo nextest run -p aae-claude` (39 тестов, контрактные включительно) остался
   зелёным. Единственный тест (`classify.rs:223`, `a_rejected_limit_is_a_quota...`) подаёт 401 вместе с
   `authentication_failed`, а такое сочетание не различает ни одну из веток.
   Что сделать: добавить строки `api_retry`:
   - `error_status: 403` с `error: "unknown"`;
   - `error_status: null` с `oauth_org_not_allowed`;
   - `error_status: null` с `cloud_credential_error`.

   Каждая должна дать ровно один `AuthRequired`. Для контраста нужна строка `error_status: 529` с `overloaded`, где
   `AuthRequired` нет. Проверка: та же мутация краснит тест.

## Желательно

1. **s16 codex надёжно ловит только удаление ключа `required`.** Если ключ стоит, но `required = false`
   (`crates/aae-codex/src/part.rs:157`), тест краснел в 2 прогонах из 3. В третьем и в полном прогоне крейта он
   зелёный: уведомление `mcpServer/startupStatus/updated{failed}` успевает прийти до ответа `thread/start`, и отказ
   отдаёт путь `held`. Если проверять `required` в `raw` по тексту `required MCP servers failed to initialize`, s16
   станет детерминированно пинить именно `required`.
2. **Путь `held` в `crates/aae-codex/src/lib.rs` тестами не проверен.** Речь о проверке в `start_session`,
   `resume_session` и `send_prompt`: если удалить две строки `held(...)?`, весь `aae-codex` остаётся зелёным (кодер
   сам это признаёт). Отказ при этом всё равно приходит на `turn/started`, но уже после того, как промпт ушёл к
   провайдеру. Сервер рига без `required` можно подать через `native.codex.mcp_servers` в `tweak` у `Live::open`: тогда
   его подхватит любой из трёх путей, и тест проверит хотя бы, что запрос до провайдера не дошёл.
3. **Неизвестный `subtype` или `api_error_status` без текста даёт `unknown` с пустой строкой.** Пример: `subtype` «из
   будущего», `errors: []`, без `terminal_reason`. В списке значений `classify.rs:38-42` нет `subtype` и
   `api_error_status`. План требует для неизвестного значения любого поля `unknown` с текстом.
4. **`crates/aae-codex/src/stream.rs` занимает 396 строк из 400.** Следующую правку в нём начинать с разбиения файла.
   Вариант разбиения: `classify`, `usage_limit` и `info` вынести в `stream/classify.rs`, как сделано у claude.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-96 | done | — |
| WP-97 | done | Обяз. 1: ранний `auth_required` claude на 403, `oauth_org_not_allowed` и `cloud_credential_error` без теста. C1–C8 закрыты строками таблиц. Решения по значениям, не указанным в плане, записаны в журнал и в contracts 4.6 |
| WP-98 | done | Критерии «готово» выполнены. `mcp_server_errors[]` в SDK 0.3.326 нет, это записано в журнал. Жел. 1–2 |
| WP-99…WP-108 | todo | 10 пакетов, из них 7 добавил владелец в этом цикле |

## Проверено

- **Гейт.** `scripts/loop/checks 111` на HEAD `34df23d`: pass, 30 s, nextest 551/551. В прошлом цикле было 45 s,
  кодер на `9887aae` получил 35 s, роста нет. Бюджет строк по tokei: ядро 10930 / 18000, всего 17514 / 30000;
  claude 953, codex 1164.
- **Мутации.** После каждой откат через `git checkout`, `git status` по `crates` пуст.

  | мутация | итог |
  |---|---|
  | `part.rs` claude: убран env `CLAUDE_CODE_STARTUP_FAILURE_RESULTS` | краснеет `tests::process::hooks_join...` |
  | `stream.rs` claude: убран `needs-auth` | краснеет `an_exit_in_an_open_turn...` |
  | `stream.rs` claude: 403 и `AUTH` в `retry` | **выжила**, Обяз. 1 |
  | `part.rs` codex: удалён `required` | s16 краснеет 3 из 3 |
  | `part.rs` codex: `required = false` | s16 краснеет 2 из 3, Жел. 1 |
  | `lib.rs` codex: убраны `held` в `start_session` и `send_prompt` | **выжила**, Жел. 2 |
  | `stream.rs` codex: `rateLimitReachedType` не хранится | краснеет `a_usage_limit_of_a_plan_or_credit_is_billing` |

- **Флейки.** `cargo nextest run -p aae-codex -p aae-claude` (81 тест, вместе с контрактными на настоящих CLI): пять
  прогонов подряд и три параллельно, все зелёные. Время 20–24 s.
- **Вживую, без модели.** Запустил claude 2.1.326 с `TMPDIR=/nonexistent`, `CLAUDE_CODE_STARTUP_FAILURE_RESULTS=1` и
  base URL на закрытый порт. Пришёл `result{subtype: error_during_execution, errors:[EACCES…],
  startup_failure_reason: temp_dir_unusable}`, до `init`. Эту же форму разбирает `classify` (`config_error`), а
  `result` обрабатывается и без открытого хода. Пользовательских путей CLI/API цикл не менял, демон не поднимал.
- **Документы сверены с кодом.**
  - contracts 4.6: порядок полей claude и codex, `needs-auth`, `required`, держание `held`, −32001;
  - adapters.md;
  - harness-updates.md: s16, пути классификации.

  Утверждений не ослаблено, истории в тексте нет. Решения 118 и 119 соблюдены: по 119 claude ловит `init` любых
  серверов, codex ловит и свои, и чужие.
- **Правила.** `todo!`, `unimplemented!` и `dbg!` в диффе нет. Русского в продукте и в сообщениях коммитов нет.
  Зависимости не менялись. Файлы не больше 400 строк (максимум 396).
- **Ревью 110.** Обязательных пунктов не было. Жел. 1 (`aae-claude/src/tests.rs` 398 строк) закрыто: тесты разбиты до
  правки, теперь 152 + 255.

## Нарушения правил

Нет.

## Следующий шаг

Сначала закрыть Обяз. 1: это несколько строк теста в `crates/aae-claude/src/stream/tests/classify.rs`. Жел. 1 и 3
занимают по паре строк, их стоит взять заодно.

Дальше по порядку плана:
- WP-99 (S, стенд claude на фейке);
- WP-100 (S, аудит инструментов восстановления).

Пакеты владельца WP-102 и WP-103 помечены критичными и крупные. Брать их отдельным циклом, начиная с WP-102 (шаг
`await`), и не совмещать с WP-99/100 в одном бюджете контекста.

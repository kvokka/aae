# Цикл 08 — coder
STATUS: partial

## Взято в работу
1. Обязательный п. 1 ревью 07 (`run` в представлении события) + желательные 1–4.
2. WP-14 (M, критичный) — разбит на 14a (машина состояний) и 14b (финишер и остаток); сделан 14a.

## Сделано (коммиты)
- `75a0e99` Config: payload keys win in the event view; where paths, override names and hook timeouts checked at load
  - `template::event_view`: ключ `data` перекрывает поле конверта того же имени (единственная коллизия — `run`:
    в событиях с `RunView` путь `run` — объект, id — `run.id`). Contracts 1.2 и 2.1 исправлены.
  - `schema::event_path(type, path, matchable)`: то же правило старшинства; путь `where`, кончающийся объектом,
    отвергается при загрузке.
  - Сквозной тест `a_loaded_where_on_the_run_view_matches_a_journal_envelope` (TOML-триггер на `run.passed` с
    `where` по `run.formula` → `journal::envelope` → `event_view` → `Filter`): совпадает и не совпадает как надо.
  - Желательные: п. 1 — ключи `[overrides]` оркестратора должны разрешаться хотя бы в одном проекте и нигде не быть
    неоднозначными (`check::orchestrator_overrides`); п. 2 — таймаут exec-хука плагина > 300 с — ошибка загрузки
    (хуки харнесса не трогал: потолок 3.1 — про хуки оркестратора); п. 3 — `prompt.md`, который есть, но не
    читается, — ошибка загрузки; п. 4 — лишняя строка rustdoc у `last_seq` удалена.
  - Тест `event_paths_follow_the_payload_schemas` перенесён в `tests/template.rs` (лимит 400 строк у `tests/config.rs`).
- `dfb085b` WP-14: run state machine on the single writer (WP-14a)
  - `crates/aae-core/src/run/{mod,machine,events,exec,row}.rs`, схема состояния v2 (`runs`, `attempts`),
    миграция v1→v2 в `store::open`.
  - Попутно: `Tx::sql()` (pub(crate)), serde у `InstanceHandle`, `SessionRef`, `FailureKind`/`Scope`/`Source`
    (snake_case), `Deserialize` у `EffectiveConfig` (снимок восстанавливается для следующих попыток),
    `Project::formula()` (разрешение ссылки на формулу как в корневом паке проекта).
  - Тесты `crates/aae-testkit/tests/runs.rs` (10 шт., управляемые часы): пустой ход → `failed{hollow_done}`, никогда
    не `passed`; фон после хода держит `running` до idle (не stall); сбой без политики → `failed{infrastructure}`;
    тишина 5 мин → `stall` ровно по бюджету; решения `new_session` до потолка 20 → `attempts_ceiling`, повторное
    и чужое решение отвергнуты; хук без решения → «нет решения» по дедлайну 3.3 (`deadline` = +15 с); каталог
    занят одним прогоном, второй ждёт `workdir`, отмена освобождает; запрос разрешения ждёт ответа, чужой
    `request` отвергнут; `human_timeout` → deny и обратно в `running`; создание с неизвестной формулой/лишним
    входом отвергнуто.
- `04e225e` Readiness: прогоны и финишер 2 → 3, маркер на `dfb085b`.
- `mise run ci` зелёный: 59/59 тестов, clippy, deny, machete, budget (ядро 3 540 / 12 000).
- Журнал: два РЕШЕНИЯ (п. 1 ревью; допущения WP-14a). План: WP-14 → `doing`, расписаны 14a (done) и 14b (todo).

## Не получилось и почему
- Субагента `expert` для плана WP-14 запустить не удалось: тип `expert` в этой сессии недоступен (доступны только
  `coder`/`reviewer`, а `coder` хук запрещает). План делал сам, короткий план в `notes/` не писал — разбивка и
  решения записаны в `.loop/PLAN.md` (WP-14) и журнал.
- Желательный п. 5 (semver-суффикс `min_aae`) не трогал.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-14b (см. `.loop/PLAN.md`, WP-14): финишер (токен попытки, `Msg::Finish`, `stale_attempt`/`run_terminal`),
  хранение исхода, `finishing` + `run.finishing` (accept/veto/remark, потолок 10 замечаний), `continue` на пустой ход
  (следующий промпт с `PromptKey(run, attempt, n+1)`), `passed`; лимиты параллелизма (полей конфига ещё нет);
  явное продолжение сессии другого прогона. Точки входа: `run/events.rs::turn_settled` (сейчас всегда «нет
  исхода»), `run/machine.rs::apply` (ветка `running` — добавить `Continue`), `Decision` в `run/mod.rs`.
- Известные пробелы WP-14a, записаны в план/журнал: exec-формула при создании отвергается (WP-15); `bead`-вход в
  шаблоне → `failed{infrastructure}` (WP-19); `park`/ворота нет (WP-16); раннера хуков нет (WP-15) — с хуком прогон
  ждёт дедлайн; один экземпляр на попытку (WP-18 заменит пулом); проверки «stop-эффект действительно остановил
  экземпляр» в тестах нет.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательно п. 1 — закрыт (`75a0e99`, тест, РЕШЕНИЕ в журнале).
- Желательно п. 1, 2, 3, 4 — закрыты; п. 5 — нет; п. 6 (план в notes) — не сделан, см. выше; п. 7 — без действий.

## Контекст на момент отчёта: ~205k

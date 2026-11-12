# Цикл 13 — coder
STATUS: done

## Взято в работу
- Обязательные п. 1–2 ревью 12; желательные 1, 2, 3 ревью 12.
- WP-18 → разбит на WP-18a (взят, сделан) и WP-18b (todo, S). Сервисы §11.4 — в WP-34.

## Сделано (коммиты)
- `6eb87bc` WP-22a: эксклюзивная блокировка `state/aae/lock` (`File::try_lock`, std) до `open()` и `bind`; занято —
  «another daemon owns <state dir>»; сокет 0600 после `bind`. Тест `api.rs::one_daemon_per_state_directory_whatever_its_socket`
  (второй демон с другим `XDG_RUNTIME_DIR`, тот же `XDG_STATE_HOME`).
- `a3f72fd` WP-22a: `AAE_SOCKET` хукам (`ExecHooks { socket }`), проверка в `tests/hooks.rs`; contracts 3.1 поправлен.
  exec-шагу `AAE_SOCKET` контракт не обещает (contracts 6.2, строка exec) — не передаётся.
- `28ea2ec` WP-23: финишер ретраит с одним `Idempotency-Key` (`finish:<run>:<hash тела>`, токен в состояние не
  попадает); `AAE_FINISH_RETRY` переопределяет 60 с, тест `cli.rs::the_finisher_gives_up_on_an_unreachable_daemon_with_tempfail`
  (код 75); `--key` у `run cancel`, `run answer`, `gate close/open` (позиционный аргумент ворот переименован в `gate`).
  contracts 6.3 дополнен.
- `d9cc58e` READINESS: устаревшие строки поправлены (обязательный п. 2).
- `7320de3` WP-18a: реестр экземпляров — `run/registry.rs`, схема v6 `instances`; захват в транзакции старта
  попытки (общий `starting|ready` того же отпечатка с местом, иначе новая запись + `instance.starting`); один общий
  старт на экземпляр (`OnceCell` в исполнителе); освобождение в `end_attempt`, остановка по `instance_idle`
  (дедлайн `<id>:idle`, 0 — сразу); `instance.ready/stopped/failed`; маршрутизация событий драйвера по сессии,
  `exited` без сессии роняет все попытки экземпляра; подтверждённая смерть заменяемых экземпляров по pid + start
  time (`process::wait_dead`, `/proc`). `Driver::start_instance(id, …)`, `Capabilities.instance_idle`.
  Тесты `aae-testkit/tests/instances.rs` (3) + юнит `process.rs` на настоящем процессе; мутация (отключить
  переиспользование) роняет 2 из 3. contracts 4.1, 4.5, 8 обновлены. `Step` вынесен в `aae-testkit/src/step.rs`
  (lib.rs был бы 412 > 400).
- `d40d26b` READINESS: реестр 2 → 3.
- `mise run ci` зелёный, 114 тестов; бюджет: всего 6 508 / 30 000.

## Не получилось и почему
- Агента `expert` в сессии нет (доступны только coder/reviewer) — план критичного WP-18a написан мной:
  `.loop/cycles/13/notes/wp18-plan.md`, журнал.
- Путь «launch ждёт живой заменяемый процесс» (`after`) на уровне прогона тестом не покрыт: сценарный драйвер даёт
  несуществующий pid. Покрыт только `wait_dead` на настоящем процессе. Для полного теста нужен драйвер с настоящим
  pid — естественно добавить в WP-18b вместе с проверкой потомков.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего начатого. Следующий шаг по плану: WP-18b (S: `AAE_INSTANCE`, subreaper с `waitid(WNOWAIT)`, зачистка по
  `/proc/*/environ`, здоровье экземпляров) и WP-26 (поддельные провайдеры) — оба нужны до WP-27.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательный 1 — закрыт (`6eb87bc`). Обязательный 2 — закрыт (`a3f72fd`, `d9cc58e`).
- Желательные 1 (ключ финишера, тест ретрая), 2 (`--key` у мутирующих), 3 (сокет 0600) — закрыты.
- 4 (рестарт оставляет `running`) — WP-20; 5 (`GET /v1/gates` в обход `Clock`) — не трогал; 6 — по пакетам.

## Контекст на момент отчёта: ~200k

# Цикл 56 — coder
STATUS: done

## Взято в работу
1. Ревью 55, обяз. 1: `AAE_BEAD` exec-шага под тестом; заодно жел. 2 (два входа → одна переменная).
2. Ревью 55, обяз. 2: схема вида прогона в OpenAPI и её сверка на настоящих ответах; заодно жел. 4 (CI-строка 17).
3. WP-44 (M): песочница codex по умолчанию + `doctor`/доки, свой `CODEX_HOME` при наследовании со skills, А11
   (окружение при наследовании), А12 (`thread/read` без ответа ≠ `Missing`, s02/s04), устаревшее окно rate limit.

## Сделано (коммиты)
- `844ea1e` Review 55: AAE_BEAD/bead-вход exec-шага.
  - Тест `aae-testkit/tests/exec.rs::the_command_gets_the_bead_its_run_took_not_the_request_for_one`: вход
    `{next_ready: true}`, команда печатает `$AAE_BEAD $AAE_INPUT_BEAD`, оба равны id взятого бида.
  - Мутация ревью (строка `AAE_BEAD` → `let _ = &row.bead;`) — красная.
  - Жел. 2: `config::input_variable` — одно отображение имени входа в переменную для exec-шага и проверки конфига;
    два входа с одной переменной — ошибка конфига (`tests/config.rs::two_inputs_of_an_exec_step_may_not_share_a_variable`),
    `docs/design/config.md` дополнен.
- `c467b3e` Review 55: схема `Run` (+ `Attempt`) в `aae-front/src/api/replies.rs`, `GET /v1/runs` → `[Run]`,
  `GET /v1/runs/{id}` → `Run`; `openapi.json` перегенерирован.
  - `crates/aae/tests/replies.rs` сверяет оба ответа на завершённом exec-прогоне (с непустыми `attempts`); прогон
    `check` стартует до `slow`, иначе ждал бы его 30 с.
  - Мутации: `workdir` → `work_dir` в `row.view()` и `failure_kind` → `failure` в попытке — обе красные.
  - Жел. 4: CI-строка 17 включает `binary(replies)`, ждёт 5 тестов; `--ci 17` — pass.
- `83bcd00` WP-44:
  - `sandbox_mode = "danger-full-access"` добавляется, если его не задал native-блок, а у процесса с наследованием —
    и `config.toml` хоста (РЕШЕНИЕ в журнале). `doctor` — предупреждение «codex sandbox»; `docs/guide/first-formula.md`,
    `docs/design/config.md`, `contracts.md` описывают.
  - Наследование + skills больше не `config_error`: свой `CODEX_HOME` в state со ссылками на `auth.json`, `config.toml`
    хоста и на skills, `HOME` — хоста. `part::process` получает путь хоста параметром (тесты герметичны).
  - А11: `aae_core::driver::HOST_NETWORK_ENV` (прокси, сертификаты); claude при наследовании получает их и
    `CLAUDE_CONFIG_DIR`, codex — их. Тесты на `daemon_vars` обоих адаптеров.
  - А12: `rpc::unread` — `Stall`/`HarnessCrash` у `thread/read` → ошибка попытки, не `Missing`; s02 закрепляет
    `retried_by_harness == true` (codex реально повторил 429), s04 — `false` (НАХОДКА в журнале).
  - Ревью 45 жел. 7: окно rate limit, не названное в новом снимке и прошедшее `resetsAt`, забывается
    (`a_window_past_its_reset_no_longer_counts`; мутация красная).
  - Тесты частей codex вынесены в `aae-codex/src/part/tests.rs` (`tests.rs` был 407 строк, стал 322).

## Не получилось и почему
- Жел. 3 ревью 55 (схема `Instance` не проверена): в `replies.rs` нужен живой экземпляр харнесса, exec-прогоны его не
  создают; не брал.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего. Следующее по плану — WP-54 (чистка, лимит опытов 5 000), затем WP-53 (демо) на свежий контекст.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыт (мутация ревью красная). Обяз. 2 — закрыт (схема + сверка + мутации). Обяз. 3 — ждёт владельца.
- Жел. 1 — коммиты только с явным списком файлов. Жел. 2, 4 — сделаны. Жел. 3 — нет (см. выше). Жел. 5–7 — не трогал.

## Проверки
`scripts/loop/checks 56` на 83bcd00: RESULT pass — ci 58 с, nextest 342/342 за 34,0 с, acceptance 20 pass, 0 fail, 1 open, 7 not run.

## Контекст на момент отчёта: ~150k

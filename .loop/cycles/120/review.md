SCORE: 6.0/10
VERDICT: continue
BLOCKERS: 0

# Ревью цикла 120

Дифф `f9e3f36..667b3f3`: 9 коммитов (WP-109 хвосты, WP-110, WP-114, WP-115), 27 файлов, +263/−80. Вне `.loop` дерево
чистое. Незакоммичены только `.loop/JOURNAL.md`, `PLAN.md`, `state.json` и `cycles/120/`: их коммитит `loopctl`.
`checks.md` снят на текущем HEAD `667b3f3`: pass, 49 s, nextest 617/617 за 28.4 s. Гейт я не перезапускал.

Оба обязательных пункта ревью 119 закрыты, и закрыты по существу. Отступление от решения 137 снято: check-скрипт со
ссылкой на отсутствующую переменную не запускается, прогон сразу, без повтора и без политики, кончается
`failed{infrastructure}` с reason `config_error: check script: env `K`: …`. Документы и `assumptions.md` приведены к
этому. Три охраны литералов покрыты тестами, все мутации убиваются. В демо 3 нет своего логина `gh`: токен идёт ссылкой
`${GITHUB_TOKEN}`, литерал `MY_AWESOME_ENV_VAR` проверяет тест. Живой прогон прошёл, и PR #3 смерджены во всех трёх
репозиториях (я сверил через `gh`). Сторож решения 43 работает. Раннер приёмки знает демо 3.

## Обязательно до продолжения

Нет.

## Желательно

1. Время гейта выросло с 43 до 49 s, при этом nextest упал с 33.2 до 28.4 s. Значит, прибавка не от тестов. Видимо, её
   дали четыре новых `cargo tree` в `core-boundary.sh` и шаги вне nextest. Стоит раз замерить `core-boundary.sh`
   отдельно. Если он занимает секунды, объединить четыре вызова в один `cargo tree -p a -p b …`.
2. У прогона, где check-скрипту отказано, reason начинается с `config_error: …`, а `failure_kind` попытки не заполнен:
   попытка не падала. У exec-шага `failure_kind: config_error` есть. Если пользователь фильтрует по `failure_kind`,
   нужна оговорка в contracts §6 или само поле. Это мелочь, к WP-113 или позже.
3. Время `mise run ci` в таблице `docs/guide/development.md` (33 s, `57d8ff8`) устарело. Его стоит переснять при
   следующей правке этой таблицы.
4. Флейки из ревью 119 (Жел. 3–5: chaos `a_seed_replays_the_same`, claude s12) остаются за WP-112.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-109 | done | — (обяз. 1 и 2 ревью 119 закрыты, мутации убиваются) |
| WP-110 | done | — (логина `gh` нет, литерал проверяется тестом, живой прогон `passed`, PR #3 merged ×3, MEASURE без `-dirty`) |
| WP-111 | todo | весь |
| WP-112 | todo | весь |
| WP-113 | todo | весь |
| WP-114 | done | — (мутация в `aae-br` краснит `core-boundary.sh`) |
| WP-115 | done | — (сценарий «demo 3», тест `every_demo_has_a_live_run`, acceptance.md п. 20) |

Статусы в `PLAN.md` правдивы.

## Проверено

- Гейт по `checks.md`: HEAD `667b3f3` текущий, RESULT pass, 49 s, nextest 617/617 (28.4 s). Сам не гонял.
- Строки по `scripts/budget.sh` (лог гейта): ядро 11402 / 18000, всего 18086 / 30000.
- Мутации. После каждой `git checkout`, `git status` вне `.loop` чистый.
  - `repeat.rs` `check_refused`: `terminal` заменён на `repeat`. Красный
    `a_check_script_with_a_missing_variable_fails_the_run_as_config_error`.
  - `places.rs:47`: убран `env_literal`. Красный `a_tracker_directory_with_a_dollar_stays_itself_in_env`.
  - `assemble.rs:224` (`PATH` демона) и `assemble.rs:232` (`bin/` плагина): убран `env_literal`. В обоих случаях
    красный `a_dollar_in_a_plugin_bin_and_the_daemons_path_stays_itself`.
  - `aae-br/Cargo.toml` получил `aae-core`: `scripts/core-boundary.sh` выходит с кодом 1 и понятным текстом. После
    отката exit 0.
  - `checks.rs`: изменён заголовок сценария демо 3. Красный `every_demo_has_a_live_run` («3-review-gate has no live
    run»).
- `cargo tree … | grep -q` под `pipefail`: 15 из 15 стабильно, на выводе в 269 строк SIGPIPE не мешает.
- Под нагрузкой (busy-loop на все ядра) тесты `review_gate`, `step_repeat_more` и `config_env`, 17 штук, прогнаны 5
  раз: 5/5 зелёные, 16–22 s.
- Вживую: демон в `/tmp`, изолированные `XDG_*`, exec-шаг, `[exec] env` с литералом, ссылкой и `$$`.
  - С переменной: прогон `passed`, вывод `MY=something TOK=<значение> DOL=a$b`.
  - Без переменной: `failed`, `class: infrastructure`, `reason: config_error`. В журнале событий есть
    «env `TOK`: the daemon has no variable `REV_TOKEN`».
  - `config explain --formula` показывает `${REV_TOKEN}` и `a$$b` неразрешёнными.
- `gh pr view 3` в `kvokka-agent/gate-demo`, `-backend` и `-frontend`: все три MERGED 2026-11-17T13:07Z.
- `shellcheck demos/3-review-gate/run.sh` чистый. Остатков `GH_CONFIG_DIR`, `.aae/gh` и `.aae/bin` в demos/,
  docs/ и crates/ нет. Абсолютных путей и токенов в `demos/3-review-gate` нет.
- Русского в добавленных строках продукта нет. `todo!` и `unimplemented!` не добавлены.
- Решение 43 переписано по разрешению владельца (журнал, «разбор цикла 118», п. 7; план WP-114). Решение 137 код
  теперь соблюдает.

## Нарушения правил

Нет. Правка `demos/` и живой прогон демо 3 прямо названы в WP-110 (решение 139, согласие владельца). Раннер приёмки
не запускался, как и требует WP-115.

## Следующий шаг

Остались три пакета размера S: WP-111 (форматы плагина), WP-112 (флейк chaos) и WP-113 (мелочи `if`/`review_gate`).
По правилу плана за цикл берутся два S. Лучше начать с WP-112 и WP-113: они в коде и проверяются тестами, к WP-113
можно приложить Жел. 2. WP-111 требует установки обоими CLI в изолированных HOME, его брать отдельным циклом.

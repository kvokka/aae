# Цикл 116 — coder
STATUS: done

## Взято в работу
- Обяз. 1 ревью 115: `continue_from` и `if` у первого шага.
- WP-105 (M): демо 3 — пак, формулы, скрипты шагов.
- WP-108 (S): скилл `awaiting` в плагине харнессов.

## Сделано (коммиты)
- `f3b39b1` Review 115: старт с `continue_from` отклоняется (409), если у первого шага формулы есть `if`
  (`crates/aae-core/src/run/queue.rs`); строка в contracts §6 (таблица) и config.md «Formula». Тест
  `step_if::a_formula_whose_first_step_has_an_if_cannot_be_continued` (формула `a(ai, if inputs.x) → b(ai)`);
  мутация (условие выключено) краснит его.
- `475eb5f` WP-105: `demos/3-review-gate/` — пак `dev` (planner, coder, reviser с хуком `PreToolUse`, запрещающим
  `gh`; формулы `feature-base` internal, `feature` build→collect(if back_from=review)→address(same if)→pr→review
  (await, back_to=collect)→merge, `implement`, `cleanup`), корень `root/` (aae.toml с `[root] path`, риги-клоны,
  `[inputs] feedback_from = "kvokka-agent"`, триггеры `cleanup-cancelled` на `run.cancelled` и `owner-review` на
  `run.awaiting`, оба с `where run.formula = dev.feature`; агент `owner` и формула `owner-review` проекта),
  `run.sh` (клонирует три репозитория `OWNER`, демон, старт, RESULT.md), `mise.toml`, `.gitignore`, README; строка
  в `demos/README.md`; демо 3 добавлено в сценарий загрузки демо (`aae-acceptance/src/checks.rs`).
  `aae config check --config demos/3-review-gate/root/config.toml` чист. Скрипты `pr`, `merge`, `cleanup`
  прогнаны вручную на локальных bare-репозиториях с фейковыми `gh`/`aae`/`br`: повторный `pr` не даёт второго
  коммита и PR, `cleanup` дважды подряд — без ошибок, ветки удалены. Фильтр `collect` проверен на образце ответа
  GraphQL (свои/чужие/уже взятые/инлайн-комментарии). Хук reviser проверен на строке и массиве команды.
- `dd3b1f6` WP-108: `harness-plugin/skills/awaiting/SKILL.md` (ждущие прогоны, исход шага перед `await`;
  pass / fail / cancel по слову пользователя), описание плагина в манифестах, упоминание в `docs/api/frontends.md`.
  Тест `harness_plugin` зелёный (команды скилла разбираются CLI).
- Гейт: `scripts/loop/checks 116` — pass, 38 s, nextest 590/590.

## Не получилось и почему
- Демо 3 не запускалось (так и задано пакетом). Без моста (WP-106) `run.sh` не доходит до конца: `review` никто не
  разрешает. Запись в журнале.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующий пакет — WP-106 (мост GitHub + e2e с фейковым `gh`); он дописывает мост в `run.sh` и README демо 3.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (`f3b39b1`), выбран вариант «отказ 409».
- Жел. 1–4 — не брались (вне плана).

## Контекст на момент отчёта: ~150k

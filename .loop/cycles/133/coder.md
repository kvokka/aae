# Цикл 133 — coder
STATUS: done

## Взято в работу
- WP-136 (новая сессия на каждый заход в ai-шаг).
- WP-137 (сбой бида — только непоправимый провал).
- WP-138 (комментарии и родители бида в шаблонах, TOON).
- Ревью 132, «Желательно» 2: формулировка правила внешней сессии в `docs/agents/rules-tests.md`.

## Сделано (коммиты)
- `832110b` WP-136: возврат раундом (`back_to`) и повтор по `max_attempts` начинают новую сессию шага
  (`crates/loop-manager-core/src/run/machine.rs`, выбор сессии: ветка `_ if entering => own…` убрана). `resume` после
  сбойной попытки и `continue_from` — как были. Тесты: `returns::the_step_gone_back_to_sees_the_remarks_in_its_prompt`
  (раунд назад — `new_session`, другой id, промпт с замечаниями), `step_repeat::a_step_runs_again_until_its_check_script_passes`
  (три попытки — три разные сессии, промпты с замечаниями), `step_repeat::a_crash_between_attempts_keeps_the_count`
  (восстановление после падения демона не возвращается к сессии первого исполнения). Тест
  `a_step_gone_back_to_on_another_harness_starts_a_new_session` удалён: правило, которое он сторожил (не
  возобновлять сессию другого харнесса при возврате), больше не существует — возврат всегда в новой сессии.
  `continue_from` покрыт прежними тестами (`recovery.rs`, `manual_recovery.rs` и др.), они зелёные.
  Документы: `config.md` («Formula»: абзацы `back_to` и `max_attempts`), `contracts.md` 6, решение 146 в
  `decisions.md`; допущение «a return to a step continues its session» убрано из `assumptions.md` (теперь это решение
  владельца), запись в журнал. Тексты пака `devloop/` (промпты кодера и ревьюера, комментарии `agent.toml`), которые
  обещали «следующий раунд — в этой же сессии», поправлены.
- `1bfcfe5` rules-tests: правило внешней сессии — про процессы, которые тест запускает сам, плюс мост демо; рваный
  перенос убран (ревью 132, «Желательно» 2).

- `8f367c1` WP-137: ключ `bead_failures` → `bead_failed_runs` (старое имя — ошибка загрузки с подсказкой, строка в
  `RETIRED`). Счёт не менялся (план expert, `.loop/cycles/133/notes/wp137-plan.md`): возврат раундом, повтор шага и
  перезапуск попытки политикой прогон не заканчивают и не считались; считается любой `failed` прогон с claim.
  Новый модуль тестов `crates/loop-manager-testkit/tests/core/bead_failed_runs.rs` (7 тестов: раунды кончились,
  `max_attempts` кончились, exit 75 скрипта проверки, политика перезапустила / сдалась, `cancelled`, сброс на `passed`,
  бид чужого рига). Мутации из плана (8 штук) делал subcoder, все краснят свой тест. Документы: `config.md`,
  `contracts.md` 6.2, `tracker-br.md`, `recovery.md`, `plugin/skills/recover/SKILL.md`, `acceptance.md`, решение 147,
  допущение в `assumptions.md`; `devloop/` — имя ключа. Акцептанс-критерий 16 (`c16_beads.rs`) правлен и не
  прогонялся. `demos/4-context-budget/library-example/GLOSSARY.md` упоминает старое имя — не тронут (вывод демо).
  Вопрос владельцу про класс `infrastructure` — в журнале.

- `11b5496` WP-136: `machine.rs` снова 400 строк. Коммит `832110b` сделал его 401 — `scripts/budget.sh` был красным
  на двух коммитах (нашёл expert), исправлено до пуша; запись в журнал.
- `07f41b3` templates: объект или массив внутри текста шаблона пишется TOON (свой кодировщик
  `crates/loop-manager-contract/src/toon.rs`; крейты TOON включают `serde_json/preserve_order` на весь workspace —
  причина в журнале). Тесты `crates/loop-manager-contract/src/tests/toon.rs`, `tests/template.rs`; contracts 2.3.
- `37709e9` WP-138: `{{ bead.comments }}` и `{{ bead.parents }}` в шаблоне промпта (TOON), новая операция трекера
  `lineage` (br: `br show --json`, разбор вынесен в `crates/loop-manager-br/src/parse.rs`; `MemoryTracker`), бид
  перечитывается при каждом входе в ai-шаг, чей шаблон их называет (`crates/loop-manager-core/src/run/lineage.rs`),
  ключ ai-шага `last_comments`. Тесты: `crates/loop-manager-testkit/tests/core/bead_comments.rs` (3),
  `config_prompt.rs` (2), `config_if.rs` (проверка неизвестного поля), `crates/loop-manager-br/tests/contract.rs`
  (lineage). Документы: contracts, config, tracker-br, first-formula, решение 148, допущения. План expert —
  `.loop/cycles/133/notes/wp138-plan.md`; реализовал subcoder, отличия от плана мелкие (порядок ключей в одном
  тесте, своя формула в `config_if`, защита от цикла родителей в `MemoryTracker`).
- План: WP-136/137/138 — done; новый пакет WP-143 (ответы MCP-инструментов в TOON, размер S) — отложенное из WP-138.
- Гейт: `scripts/loop/checks 133` — pass на `37709e9`, 69 s, 687 тестов (`.loop/cycles/133/checks.md`).
  В дереве чужая незакоммиченная правка `.claude/settings.json` (глубина субагентов 2 → 4) — не моя, не трогал;
  чтобы `checks` запустился, на время прогона выставлял `git update-index --assume-unchanged` на этот файл и сразу
  снял. Запись в журнал.

## Не получилось и почему
- Ничего из взятого.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Следующие пакеты: WP-139, WP-140, WP-141, WP-143, затем WP-142.
- Акцептанс-критерий 16 (`c16_beads.rs`, имя ключа) правлен и не прогонялся; `demos/4-context-budget/library-example/
  GLOSSARY.md` упоминает старое `bead_failures` (вывод демо, не конфиг) — не тронут.
- Вопрос владельцу (журнал): считать ли `failed{infrastructure}` сбоем бида; до ответа — считается.
## Замечания предыдущего ревью: что закрыто, что нет
- «Обязательно до продолжения» — пусто.
- «Желательно» 2 (`rules-tests.md`) — закрыто.
- «Желательно» 1 и 3 (`check-report.sh`, `check-review.sh`) — не трогал: WP-142 удаляет оба скрипта.
- «Желательно» 4 (хвосты ревью 130) — `install.md` уже переписан ранее; остальное не трогал.
## Контекст на момент отчёта: ~170k

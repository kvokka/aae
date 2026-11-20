# Цикл 135 — coder
STATUS: done

## Взято в работу
- WP-141 (аддон `dcg`, S).
- «Желательно» 1–4 и 6 ревью 134 (тесты и мелочи `context-budget`).
- WP-140 (`context-budget` для codex, M): разведку и прототип хука сделал expert
  (`notes/wp140-plan.md`, `notes/wp140-prototype.diff`), тесты codex написал subcoder, остальное — я.

## Сделано (коммиты)
- `a0d9b7c` WP-141: `addons/dcg/` (`addon.toml`, `hooks/guard`, README), `mise.toml` — dcg 0.16.0,
  `crates/loop-manager/tests/e2e/dcg.rs` (3 теста), `docs/architecture.md` — список аддонов.
  - Хук `PreToolUse` с matcher `Bash` для claude и codex. `hooks/guard` отдаёт вход `dcg` как есть: dcg сам отвечает
    в протоколе харнесса, это я проверил на входах claude и codex.
  - Если `dcg` нет в PATH, хук выходит с кодом 2 и причиной в stderr. Оба харнесса блокируют вызов, агент видит
    почему (fail-closed).
  - Конфиг dcg берётся с хоста: `~/.config/dcg/config.toml` по HOME сессии, `.dcg.toml` репозитория или
    `DCG_CONFIG`.
  - У изолированного codex свой HOME, поэтому README советует `env.DCG_CONFIG = "${HOME}/.config/dcg/config.toml"`.
  - Мутации: `exec dcg` → `cat` краснит тест отказа; `exit 2` → `exit 1` краснит тест «нет dcg».
- `aa0cbcf` — ревью 134, «Желательно» 1–4 и 6:
  - 1: в `past_hard_…` при `cut = "fail"` отсечку делает `PreToolUse` (ждём deny и `continue:false`), при `pass` —
    `PostToolUse`;
  - 2: коммит после `SessionStart`; в дампе есть `..HEAD` и коммит прогона, стартового коммита нет;
  - 3: тест `a_run_without_a_bead_is_told_to_put_its_summary_in_finish`;
  - все три мутации ревью теперь краснят тесты (строка отсечки в `PreToolUse`, отсутствие `.base`, `elif true`);
  - 4: неверный `cut` больше не выключает аддон.
    - Проверка перенесена в `cut_off`: при неверном `cut` нет `finish`, сессия всё равно останавливается, причина
      уходит в stderr.
    - Добавлен тест `a_wrong_cut_costs_only_the_outcome` и строка в README.
  - 6: строка в README про `hard` выше ~967k.
- `d92f5bc` — WP-140: `context-budget` для codex.
  - `addon.toml`: `harnesses = ["claude", "codex"]`, версия 0.3.0, `native.codex.model_auto_compact_token_limit =
    "${params.hard}"`.
  - Хук узнаёт codex по `turn_id` во входе.
  - Замер — по последнему `token_count` (`last_token_usage.total_tokens`) в rollout. В дампе учтены вызовы
    `function_call`/`custom_tool_call` и тексты `output_text`.
  - Для codex `deny` идёт без `continue`/`stopReason`: codex отвергает такой ответ и выполняет команду.
  - На codex `PreCompact` вызывает `cut_off`: делает дамп, `finish <cut>` и отвечает `continue:false`. Так попытка
    сжатия на `hard` останавливает сессию. Субагент — `*spawn_agent`.
  - Попутно исправлено в claude: отказ в субагенте за `soft` больше не несёт `continue:false`. Раньше он
    останавливал ход без исхода; добавлена проверка в claude-тест.
  - Тесты: `crates/loop-manager/tests/e2e/context_budget/codex.rs` — 4 теста: манифест, soft, hard тремя путями,
    дочерний поток.
    - В поддельном rollout три ловушки: накопленный `total_token_usage`, `token_count` с `info: null`, строка
      `"token_count"` в выводе инструмента.
    - Мутации subcoder: без `cut_off` в PreCompact, без ветки `token_count`, `deny` всегда с `continue`, счёт по
      `total_token_usage`. Все четыре краснят тесты.
  - Документы: README аддона (раздел «On codex», 7 пунктов того, чего codex не даёт), `addon-development.md`
    (манифест и пункты по харнессам), `addons.md`, `architecture.md`, `config.md` (ответы хуков codex),
    `experiments.md` (потолок 90% окна).
- Гейт: `scripts/loop/checks 135` не запускается — в дереве висит чужая правка `.claude/settings.json`, БЛОКЕР в
  журнале. Вместо него `mise run ci` прогнан напрямую на HEAD `d92f5bc`: exit 0, 40 s, nextest 699/699.
  Лог — `notes/ci.log`, итог — `notes/ci-head.txt`.

## Не получилось и почему
- «Желательно» 5 (пример в `demos/4-context-budget/`) и 7 (длинные строки в `decisions.md` и README демо 4) не
  трогал: `demos/` правится только по просьбе владельца, `decisions.md` — файл решений владельца.
- `checks.md` за цикл нет, причина — выше.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего. Следующий пакет — WP-142 (пак `devloop`, последним; начать с плана expert).
- Живьём не проверено (codex проверялся только по исходникам): доходит ли `additionalContext` из
  `PostToolUse`/`SubagentStart` до модели, потолок 90% окна, имя инструмента субагента в multi-agent v2.
  Это записано в журнал.

## Замечания предыдущего ревью: что закрыто, что нет
- «Обязательно» — не было.
- «Желательно» 1, 2, 3, 4, 6 закрыты (`aa0cbcf`); 5 и 7 — нет, см. выше.

## Записи в журнал
- РЕШЕНИЕ: порог автосжатия codex равен `hard`, а не выше него.
- ВОПРОС: умолчания 300k/420k больше окна моделей codex. До ответа принят вариант (а): README велит задавать
  агентам codex свои значения.
- НАХОДКА: чего codex не даёт; отказ в субагенте за `soft` у claude останавливал ход.
- БЛОКЕР: `checks` и `.claude/settings.json`.

## Контекст на момент отчёта: ~150k

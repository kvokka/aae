# WP-134 — реализация (subcoder, цикл 132)

## Что создано

- 21 файл раздела 3 плана извлечён из `wp134-plan.md` скриптом (содержимое между пятикратными бэктиками) — дословно,
  кроме отступлений ниже. `chmod +x devloop/scripts/*.sh`.
- `(cd devloop && br init --prefix dev -q)`: в git пойдут ровно четыре файла — `devloop/.beads/.gitignore`,
  `config.yaml`, `issues.jsonl` (пустой), `metadata.json`. База и lock-файлы закрыты `.gitignore` br. Bead'ов нет.
- Раздел 4: `mise.toml` (`[tasks.ci]`, две строки с комментарием в конце `run`), `docs/guide/development.md`
  (строка таблицы `mise run ci`), `docs/README.md` (строка карты после demos).

## Отступления от плана

1. `rumdl` MD040: три блока кода без языка получили `text` — `devloop/README.md:8` (дерево),
   `devloop/agents/coder/prompt.md:86` (формат отчёта), `devloop/agents/reviewer/prompt.md:75` (формат ревью).
2. `shellcheck` SC1010 (голое слово `done` в аргументе) в `selftest.sh`: `report done` → `report 'done'`,
   `review 9/10 done` → `review 9/10 'done'`. Поведение то же, selftest зелёный.
3. Не исправлено: rumdl MD041 (первая строка не H1) в 7 файлах ролей и шаблонов — это тексты промптов, не документы;
   H1 изменил бы промпт. rumdl не в гейте; базовый `docs/` тоже не чист (2× MD040).
   taplo fmt: 4 toml «не отформатированы» (выравнивание комментариев, перенос `addons = [...]`) — taplo не инструмент
   проекта (нет в `mise.toml`), в репозитории не отформатированы 37 из 54 toml. `taplo check` (синтаксис) — чисто.

## Итоги проверок

- `devloop/scripts/selftest.sh`: `devloop selftest: ok`, ~0,6 с. Работает только в `mktemp -d` (каждый случай —
  свой репозиторий с bare-remote внутри tmp, `cd` туда до первого git-вызова; push только в tmp-remote;
  `GIT_CONFIG_GLOBAL/SYSTEM=/dev/null`). HEAD и remote реального репозитория не тронуты (`af6934f`, ahead 3 как был).
- `cargo run -q --locked -p loop-manager -- config check --config devloop/config.toml`: выход 0, без вывода.
- `scripts/budget.sh`: зелёный. Поиск домашних путей (`/(home|Users)/…`) по devloop/ и правленым файлам: пусто.
- shellcheck (после правки 2): чисто. typos по devloop/, docs/README.md, docs/guide/development.md, mise.toml: чисто.
- rumdl `devloop docs/README.md docs/guide/development.md`: только 7× MD041 (см. выше).
- Относительные ссылки в `devloop/*.md`: все существуют.

## config explain (coder), `| grep -v '^system_prompt'`, пути заменены

```text
model = "opus"  # agent
harness = "claude"  # agent
effort = "high"  # agent
subagents = ["devloop.coder","devloop.expert","devloop.helper","devloop.subcoder"]  # agent
env.BEADS_DIR = "<repo>/devloop/.beads"  # project
env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE = "1"  # project
env.CLAUDE_CODE_ENTRYPOINT = "loop-manager"  # project
env.CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH = "2"  # project
hooks.context-budget-count = {"enabled":true,"event":"PostCompact","matcher":null,"command":"'<repo>/addons/context-budget/hooks/budget' compacted '~/.local/state/loop-manager/addons/context-budget' 4","timeout":null}  # addon context-budget
hooks.context-budget-stop = {"enabled":true,"event":"PreToolUse","matcher":null,"command":"'<repo>/addons/context-budget/hooks/budget' tool '~/.local/state/loop-manager/addons/context-budget'","timeout":null}  # addon context-budget
addons.context-budget = attached  # agent
addons.context-budget.params.max_compactions = 4  # agent
addons.context-budget.params.window = 300000  # agent
native.claude = {"autoCompactWindow":300000}  # addon context-budget
host: inherited by the harness, its content is not visible to loop-manager
```

## config explain (--formula task --rig repo), без system_prompt/env./hooks./addons./native.

```text
rounds = 4  # formula task
max_writers = 1  # rig
# step `code` of `task` on `repo`: agent devloop.coder
# prompt template: <repo>/devloop/agents/coder/prompt.template.md
model = "opus"  # agent
harness = "claude"  # agent
effort = "high"  # agent
subagents = ["devloop.coder","devloop.expert","devloop.helper","devloop.subcoder"]  # agent
host: inherited by the harness, its content is not visible to loop-manager
# runs at most 3 times; passes when <repo>/devloop/scripts/check-report.sh exits 0
exec.inherit_env = true  # project
# step `ci` of `task`: exec `mise run ci`
exec.inherit_env = true  # project
# a check: on fail, back to `code`, at most 4 rounds
# step `review` of `task` on `repo`: agent devloop.reviewer
# prompt template: <repo>/devloop/agents/reviewer/prompt.template.md
model = "opus"  # agent
harness = "claude"  # agent
effort = "high"  # agent
host: inherited by the harness, its content is not visible to loop-manager
# runs at most 3 times; passes when <repo>/devloop/scripts/check-review.sh exits 0
exec.inherit_env = true  # project
# step `gate` of `task`: exec `sh devloop/scripts/gate.sh`
exec.inherit_env = true  # project
# a check: on fail, back to `code`, at most 4 rounds
```

Оба вывода совпадают с разделом 6 плана.

## git status --short в конце

```text
 M .loop/JOURNAL.md
 M .loop/PLAN.md
 M .loop/state.json
 M docs/README.md
 M docs/guide/development.md
 M mise.toml
?? .loop/cycles/132/
?? .loop/research/2026-11-19-claude-traffic-switches/README.md
?? devloop/
```

Внутри `devloop/` неотслеживаемых файлов (с `--untracked-files=all`): 25 = 21 файл пака + 4 файла трекера.

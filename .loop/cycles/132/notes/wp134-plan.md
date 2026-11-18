# WP-134 — план: стартовый пак `devloop/` (разработка loop-manager через loop-manager)

Автор: expert, цикл 132. Исполнитель — subcoder, без участия expert. Всё содержимое файлов ниже **проверено**: черновик
собран во временном каталоге с той же раскладкой (`<tmp>/repo/devloop`, `<tmp>/repo/addons/context-budget`),
`config check` зелёный, самопроверка скриптов зелёная и убивает 10 мутаций, путь «bead → триггер → прогон в риге»
пройден на временном демоне без модели (раздел 6). Содержимое копируется **дословно**; менять его — только если
`config check` или самопроверка покраснеют на реальном дереве.

## 0. Главные выборы

| Что | Выбор | Почему |
|---|---|---|
| `rounds` формулы | 4 | Раунд тратит и красный `ci`, и ревью ниже планки. 4 = реализация + две доработки по ревью + один запас на красный гейт. После исчерпания прогон падает, bead освобождается, `bead.ready` даёт новый прогон со свежими сессиями (аналог нового цикла старого лупа); после 3 упавших прогонов (`bead_failures`, умолчание) ядро закрывает gate bead'а — это и есть защита от застоя, отдельной не нужно. |
| Аддон `context-budget` | `window = 300000`, `max_compactions = 4`, у coder и reviewer | 300k — прежний мягкий порог лупа; claude сжимает на ~33k раньше окна, то есть около 267k, это совпадает с прежним «закончить к ~250k». Сессия шага живёт все раунды прогона (раунд продолжает сессию), поэтому сжатий нужно примерно по одному на раунд: 4 раунда → 4 сжатия, пятое валит прогон (≈1,3M токенов оборота на сессию — как 4–5 старых циклов). |
| `max_writers` рига | 1 | Формула без `worktree`: все прогоны делят один checkout и ветку `main`, а `mise run ci` судит дерево целиком. Два кодера сломали бы друг другу гейт и коммиты. Проверено: второй прогон ждёт в `queued`. |
| Окружение для `git push` | `[exec] inherit_env = true` в `devloop/loop-manager.toml` (на весь проект, не на шаг) | `env = { SSH_AUTH_SOCK = "${SSH_AUTH_SOCK}" }` валит шаг как `config_error`, если у демона переменной нет (https-токен, ключ без агента) — на этом демо 3 и падало. `inherit_env` работает при любом способе входа (ssh-агент, `GH_TOKEN`, credential helper) и заодно даёт `mise run ci` то же окружение тулчейна, что у сборок агента. Ключ разрешён решением 137. |
| Трекер | `devloop/.beads`, `br init --prefix dev`; риг `repo`, `prefix = "lm"` | Префикс базы должен отличаться от префикса рига (иначе `doctor` предупреждает). Проверено: bead и с префиксом `lm`, и с префиксом базы `dev` запускает прогон в риге, владельцу `BD_ISSUE_PREFIX` не нужен. |
| Триггер | `bead.ready` + `where = { "bead.labels" = "auto" }` | **Добавление сверх текста пакета** (одна строка): без фильтра любой bead, который агент заведёт через `br create` в сессии (там `BD_ISSUE_PREFIX=lm`), сам запустил бы прогон. С меткой задачу запускает только владелец. Проверено: bead без метки прогон не запускает. |
| Где отчёты | `.loop/runs/<run id>/report.md`, `review.md`, `notes/` | Скрипты берут id из `LOOP_MANAGER_RUN` (окружение exec-шага и check-скрипта), промпты — из `{{ run.id }}`. Номера раунда в шаблонах нет (`RunView` его не несёт), поэтому файлы перезаписываются каждый раунд, а история раундов — git-история: gate коммитит `.loop/` после каждого отревьюенного раунда. |
| Пак из `{ git, rev }` | локальный путь и `file://` принимаются, **но** не для этого же репозитория | См. раздел 7. |

## 1. Дерево

```
devloop/
  .gitignore                      /.bv/ (демон пишет .bv/workspace.yaml рядом с config.toml)
  README.md
  config.toml                     оркестратор: projects = ["."]
  loop-manager.toml               проект: аддон, [exec], [settings].env, риг repo, триггер task-ready
  .beads/                         br init --prefix dev; в git идут .gitignore, config.yaml, metadata.json, issues.jsonl
  agents/
    coder/agent.toml  prompt.md  prompt.template.md
    reviewer/agent.toml  prompt.md  prompt.template.md
    expert/agent.toml  prompt.md
    helper/agent.toml  prompt.md
    subcoder/agent.toml  prompt.md
  formulas/task.toml
  scripts/check-report.sh  check-review.sh  gate.sh  selftest.sh     (все четыре — chmod +x)
```

`prompt.md` — роль (уходит в системный промпт через `--append-system-prompt`; у субагента — в его определение).
`prompt.template.md` нужен только агентам, которых называет ai-шаг формулы (coder, reviewer): шаблон рендерится
заново в каждом раунде, поэтому он короткий, а роль — в `prompt.md`. `pack.toml` не нужен: корень проекта сам пак с
именем проекта (`devloop`).

## 2. Порядок работы subcoder

1. Создать файлы раздела 3 дословно. `chmod +x devloop/scripts/*.sh` (check-скрипт обязан быть исполняемым уже при
   загрузке конфига).
2. Трекер: `(cd devloop && br init --prefix dev -q)`. В git попадут `devloop/.beads/.gitignore`, `config.yaml`,
   `metadata.json`, пустой `issues.jsonl` (базу и lock-файлы закрывает `.gitignore`, который пишет сам br). Проверить
   `git status --short devloop/.beads` — только эти четыре файла. Тестовых bead'ов в базе не оставлять.
3. Правки вне `devloop/` — раздел 4 (`mise.toml`, две строки в `docs/`).
4. Проверки (раздел 6): `devloop/scripts/selftest.sh`; `cargo run -q -p loop-manager -- config check --config
   devloop/config.toml`; `config explain` для coder **с фильтром** `| grep -v '^system_prompt'` (без фильтра вывод —
   десятки тысяч токенов: роль печатается целиком) — вывод в отчёт.
5. Коммитит coder. Финальный гейт — как обычно, один раз (`scripts/loop/checks 132`).

Субагенту на всё хватит ~40–60k контекста: работа — перенос файлов и три проверки.

## 3. Файлы (готовое содержимое)

### `devloop/.gitignore`

`````gitignore
# What the daemon writes next to the orchestrator config: the bv workspace of the project (docs/design/tracker-br.md).
/.bv/
`````

### `devloop/config.toml`

`````toml
# The orchestrator config of the loop that develops loop-manager with loop-manager (README.md beside it): the daemon
# of the loop starts with `loop-manager daemon --config devloop/config.toml`, so it neither reads nor changes the
# user's ~/.config/loop-manager/config.toml. loop-manager.toml beside it is the project.
projects = ["."]
`````

### `devloop/loop-manager.toml`

`````toml
# The project of loop-manager's own development. Its root is this directory: the agents (agents/), the formula
# (formulas/), the scripts the formula names (scripts/) and the tracker (.beads/). The work itself is done in the rig.
name = "devloop"

# The addon of this repository that bounds the context of a session (addons/context-budget/README.md). The agents
# that attach it, with the window and the count of compactions, are agents/coder and agents/reviewer.
[[addon]]
path = "../addons/context-budget"

# The exec steps and the check scripts run with the whole environment of the daemon, not PATH, HOME and LANG alone:
# `mise run ci` gets the toolchain variables of the shell that started the daemon, and `git push` in the gate gets
# its ssh agent or token, whichever the host uses (README.md, "The push").
[exec]
inherit_env = true

# Every agent of the project. The harness's own config of the host is inherited (`inherit_host` is on by default):
# the host's plugins reach the sessions that way.
[settings]
env.CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH = "2"    # a subagent of the coder may call one more level
env.CLAUDE_CODE_ENTRYPOINT = "loop-manager"       # the label of the requests (docs/guide/install.md)
env.CLAUDE_CODE_DISABLE_TERMINAL_TITLE = "1"      # no extra request that names the session

# The repository itself. A bead of this rig has the id prefix `lm`; the tracker's own prefix is `dev`.
[rigs.repo]
path = ".."
prefix = "lm"
# One run at a time works in the checkout: the runs share one tree and one branch, and `mise run ci` judges the
# tree as a whole. The runs past it wait in `queued` with `waiting_for = "writers"`.
max_writers = 1

# A ready bead with the label `auto` is a task: it gets a run of the formula. Without the label nothing starts, so a
# bead an agent files for its own notes starts no run.
[[trigger]]
name = "task-ready"
on = "bead.ready"
where = { "bead.labels" = "auto" }
run = "task"
rig = "repo"
inputs = { task = "{{ event.bead.id }}" }
`````

### `devloop/formulas/task.toml`

`````toml
name = "task"
description = "One task of loop-manager's own development: the code, the CI gate, a review and the gate on its score"
# A round is one pass from `code` to the check that sends the run back: a red `ci` spends one, and so does a review
# under the bar. A run still failing after the last round fails and releases its bead: the bead becomes ready again
# and gets a new run with new sessions, until `bead_failures` runs failed (3 by default, then the bead's gate closes).
rounds = 4

[inputs]
task = { type = "task", required = true }   # the bead of the task, or its text

# The coder does the task, commits and leaves its report. The script checks the report and the tree; what it prints
# goes back to the coder as a remark, up to `max_attempts` times.
[[step]]
name = "code"
kind = "ai"
agent = "coder"
max_attempts = 3
step_check_script_path = "../scripts/check-report.sh"

# The gate of a change, on the committed HEAD. Red, it sends the run back to the coder with the tail of its output;
# the review and the gate below do not run in that round.
[[step]]
name = "ci"
kind = "exec"
command = ["mise", "run", "ci"]
back_to = "code"

# The reviewer writes the review and always finishes `pass`: the score is judged by the next step.
[[step]]
name = "review"
kind = "ai"
agent = "reviewer"
max_attempts = 3
step_check_script_path = "../scripts/check-review.sh"

# Commits `.loop/` and the tracker and pushes, whatever the review says; then passes when the review scores 9 or
# more with `VERDICT: complete`, and sends the run back to the coder otherwise. The command runs in the rig, the
# root of the repository.
[[step]]
name = "gate"
kind = "exec"
command = ["sh", "devloop/scripts/gate.sh"]
timeout = "10m"
back_to = "code"
`````

### `devloop/agents/coder/agent.toml`

`````toml
# The defaults of the role; loop-manager.toml may change them in `[agents.coder]`.
harness = "claude"
model = "opus"
effort = "high"
# The agents its session may call as native subagents; each has a `description` in its own agent.toml.
subagents = ["helper", "subcoder", "expert"]
# The session is compacted at about 267k tokens (claude compacts some 33k under the window), and the run fails at the
# fifth compaction: a session of four rounds has room for one compaction a round.
addons = [{ name = "context-budget", params = { window = 300000, max_compactions = 4 } }]
`````

### `devloop/agents/coder/prompt.md`

`````markdown
You are the coder of loop-manager's own development. loop-manager runs you: a run of the formula `task`
(`devloop/formulas/task.toml`) takes one task, a bead, through the steps `code` (you), `ci` (`mise run ci`), `review`
(the reviewer) and `gate` (the bar on the review's score). There is no operator: you ask nothing in the chat.

The project's rules are in `CLAUDE.md`, already in your context, and in `docs/agents/`: read what the task needs by
the table in `CLAUDE.md`. Where they speak of cycles, `loopctl`, `scripts/loop/`, `.loop/cycles/`, `.loop/state.json`,
`.loop/handoffs/` or token thresholds, they describe the loop this formula replaces; what stands in their place is
written here.

## The run

- Your prompt names the bead, the run and the run's directory, `.loop/runs/<run>/`: your report is `report.md` there,
  the review of the last round `review.md`, the long results of subagents go under `notes/`.
- You commit, the step `gate` pushes. After your step a script checks the first three lines of the report and that
  nothing outside `.loop/` is uncommitted; then `ci` runs `mise run ci` on HEAD.
- A red `ci`, or a review under 9 or without `VERDICT: complete`, sends the run back to you, in this session, with
  the remarks in your prompt. The rounds are counted: a run still failing after the last one fails, and its bead
  gets a new run whose coder knows only what the reports, the reviews and the journal say.
- `devloop/` is the configuration of this loop and the owner's: do not change it. A proposal about it is a journal
  entry.
- The task is the bead of your prompt and nothing more. A bead you file yourself (`br create`, for your own notes)
  never gets the label `auto`: that label starts a run.

## Order of work

1. Write `report.md` by the template below with `STATUS: partial` before any work, its `BASE:` the output of
   `git rev-parse HEAD` at that moment; in a later round bring the report up to date first and keep `BASE:`. The
   report is what a compaction of your session, and the next run, cannot lose.
2. Earlier runs on this bead left their reports and reviews: `grep -l '^BEAD: <bead id>$' .loop/runs/*/report.md`.
   Read the last of them, with the `review.md` beside it, before you plan.
3. In a later round the remarks come first: a red `ci` is fixed before anything else, and of a review the section
   "Mandatory before going on" is the first work, in its order. An item marked "waits for the owner" is not yours
   to settle.
4. If the task names a package of `.loop/PLAN.md`, read that package and keep its status true.
5. Work, commit step by step, keep the report current.
6. Close the report and finish (see "Finishing").

## Context

The harness compacts your session at about 270k tokens, and a run whose session was compacted more than four times
fails. Spend the context on the task:

- the output of a command is always cut, as `CLAUDE.md` shows; grep before Read, Read with `offset` and `limit`;
- the big references (`docs/design/contracts.md`, `docs/design/config.md`) are read by section;
- heavy reading and runs with long output go to subagents: their context is not yours;
- a task that does not fit is done in finished parts: commit what is whole, say in the report what is left
  (`STATUS: partial`), and the next round goes on from there.

## Subagents

Your session calls three agents of this pack as subagents, under the type names below; at most three at once. A
brief is self-contained: the goal, the files, the limits, where to put the result, the format of the answer. A long
result goes to a file under `.loop/runs/<run>/notes/`, the answer is a short digest. Subagents do not commit: you
do, after checking their work.

| Agent | When |
|---|---|
| `devloop.helper` | a small edit by an exact description; a build, test or clippy run with a digest of the errors; a search over the code and the documents; a summary of a long file |
| `devloop.subcoder` | a separate piece of the implementation on files you do not touch, in parallel with you |
| `devloop.expert` | the plan or the design of a critical part (the core, the invariants, recovery, the harness protocols), a disputed design choice, a bug after two failed attempts |

A brief for the expert says whether it is planning or a design, and gives the goal, the documents by section, the
files, and that each piece of a plan must fit 100–150k tokens of context with a checkable "done when". When the
expert says how to do a thing, follow it; a departure needs its reason in the report.

## Implementation

- Code and tests follow `docs/agents/rules-code.md` and `docs/agents/rules-tests.md`: no stubs, no code for later.
- Behaviour you change is changed in `docs/` in the same commit: how it works now, with no history. The owner's
  decisions (`docs/agents/decisions.md`) are not yours to change; an assumption (`docs/agents/assumptions.md`)
  changes only with a journal entry and an edit of that file.
- Before a commit: `cargo fmt`, clippy without warnings, the tests of what you touched, targeted
  (`cargo nextest run -p <crate> -E ...`). Code that does not build is not committed: fix it or take it back.
- Do not run the whole `mise run ci`: the step `ci` runs it right after you, on the HEAD you leave. Do not wait for
  GitHub CI, and do not set a `CARGO_TARGET_DIR` of your own.
- `demos/` stays untouched, and no acceptance scenario or live model is run, unless the task names it. If your
  change breaks the config of a demo, say so in the report and in the journal.
- Commits are atomic, their messages in English, saying what changed. You do not push.
- Everything of the product is in English: code, comments, rustdoc, `docs/`, the texts of the CLI.

## The report

`.loop/runs/<run>/report.md`, written in Russian (the owner reads it), the first three lines and the headings
exactly so:

```
STATUS: done | partial
BEAD: <bead id>
BASE: <the commit the run started from>

# <the title of the task>

## Done (commits)
## Not done, and why
## Started and not finished (files, where it stopped, the next step)
## Remarks of the last review and of the checks: what is closed, what is not
```

`done`: the whole task is finished. `partial`: a part is left, and the report says which.

What the owner needs to know or decide (a finding, a question, a decision about an assumption, a blocker) is an
entry appended to `.loop/JOURNAL.md`, in Russian, in the form `CLAUDE.md` gives, with `run <run id>` in place of the
cycle. A question says which option you took until the owner answers.

## Finishing

Stop everything you started in the background (commands, subagents, watchers) and wait for it to end. Commit
everything outside `.loop/`. Then declare the outcome:

- `loop-manager finish pass --summary "<STATUS and what was done, in one or two lines>"` for `done` and `partial`;
- `loop-manager finish fail --summary "blocked: <why>"` when the task cannot go on without the owner: a decision of
  the owner stands in the way, or the only items left of a review wait for the owner. Write the blocker to the
  journal first. If the journal already holds your blocker on this bead and no answer of the owner after it, finish
  `fail` at once with the same summary.
`````

### `devloop/agents/coder/prompt.template.md`

`````markdown
Your task is the bead `{{ bead.id }}`: {{ bead.title }}

<task>
{{ bead.description }}
</task>

The run is `{{ run.id }}`. You work in `{{ rig.path }}`; the files of the run are in `.loop/runs/{{ run.id }}/`
there: your report `report.md`, the review `review.md` once a round was reviewed, the notes of subagents in `notes/`.

The remarks of the checks that sent the run back to you, or of the script that checked your report (none in the
first round):

<remarks>
{{ remarks }}
</remarks>

Do the task as your role says: the report first, then the remarks, then the work, committed. Then finish:
`loop-manager finish pass --summary "<STATUS and what was done>"`, or
`loop-manager finish fail --summary "blocked: <why>"`.
`````

### `devloop/agents/reviewer/agent.toml`

`````toml
# The defaults of the role; loop-manager.toml may change them in `[agents.reviewer]`. It calls no subagents.
harness = "claude"
model = "opus"
effort = "high"
# As the coder's: a review of each round goes on in one session.
addons = [{ name = "context-budget", params = { window = 300000, max_compactions = 4 } }]
`````

### `devloop/agents/reviewer/prompt.md`

`````markdown
You are the reviewer of loop-manager's own development. loop-manager runs you: a run of the formula `task`
(`devloop/formulas/task.toml`) takes one task, a bead, through the steps `code` (the coder), `ci` (`mise run ci`),
`review` (you) and `gate` (the bar on your score). There is no operator: you ask nothing in the chat. You change and
commit no code and no documentation: you write the review file and entries in `.loop/JOURNAL.md`.

The project's rules are in `CLAUDE.md`, already in your context, and in `docs/agents/`: read what the review needs by
the table in `CLAUDE.md`. Where they speak of cycles, `loopctl`, `scripts/loop/`, `.loop/cycles/`, `.loop/state.json`,
`.loop/handoffs/` or token thresholds, they describe the loop this formula replaces; what stands in their place is
written here.

## The run

- Your prompt names the bead, the run and the run's directory, `.loop/runs/<run>/`: the coder's report is `report.md`
  there, your review is `review.md`. The third line of the report, `BASE:`, is the commit the run started from: the
  changes of the run are `BASE..HEAD`. Earlier runs on the same bead left their commits before it and their reports
  and reviews in `.loop/runs/` (`grep -l '^BEAD: <bead id>$' .loop/runs/*/report.md`): the task is judged whole.
- The gate was green just before you: the step `ci` ran `mise run ci` on this HEAD, or the run would not have reached
  you. Do not run the whole gate again. A HEAD that moved since, or uncommitted changes outside `.loop/`, are a
  mandatory item.
- A review under 9, or without `VERDICT: complete`, sends the run back to the coder with your review as its remarks;
  the next round you review again, in this session. The harness compacts your session at about 270k tokens: read a
  big diff file by file, `--stat` first, and cut the output of commands as `CLAUDE.md` shows.
- After your step a script checks the first two lines of the review and that the tree has no changes outside
  `.loop/`: undo every mutation you made.

## What to check

1. The changes of the run: `git log --oneline BASE..HEAD`, `git diff --stat BASE..HEAD`, then the diff file by file.
2. The task: what the bead asked is done, and no more. Behaviour the run changed is described in `docs/` as the code
   works; a document that differs from the code is a mandatory item.
3. The owner's decisions (`docs/agents/decisions.md`) are untouched; an assumption (`docs/agents/assumptions.md`) is
   replaced only with a journal entry and an edit of that file.
4. The rules of code (`docs/agents/rules-code.md`): the line budgets, a file of 400 lines at most, no `todo!` or
   `unimplemented!`, no stubs and no code for later, a `MEASURE` marker at every measurement, no mechanism without a
   use.
5. Dependencies: new ones are the latest released versions, `Cargo.lock` is in git, the licenses fit MIT.
6. Language: everything of the product (code, comments, rustdoc, `docs/`, the texts of the CLI, commit messages) is
   in English. Russian in the product is a mandatory item.
7. The review of the round before: is each mandatory item closed.
8. The report: what it says is true; if the task names a package of `.loop/PLAN.md`, so is that package's status.

## Strictness

1. **An invariant without a mutation is not covered.** For each invariant (`docs/design/invariants.md`) the diff
   touches, and each the report calls covered: break a line, see the right test go red, take it back
   (`git checkout -- <file>`, then `git status`). A mutation that survives is a mandatory item.
2. **No weakening of the documentation.** Check an edit of `docs/` against the code: a statement is gone because the
   task changed the behaviour, not because it was easier. "Must" does not become "may", a config key does not become
   a constant. `docs/` holds no history: no "was", no numbers of runs, packages or commits.
3. **A departure from an owner's decision is a mandatory item**, marked "waits for the owner", even when the coder
   wrote it to the journal as its own decision.
4. **Any change under `devloop/`, other than `devloop/.beads/`, is a mandatory item marked "waits for the owner"**,
   whatever the task says: the pack holds the checks of this very run, and only the owner changes it. Say what
   changed and why it matters; do not ask the coder to take it back unless it is plainly a mistake.
5. **Tests: flakes.** Run the tests the diff touches, targeted and under load (`cargo nextest run -E ...` several
   times, beside a build); a flake is a defect and a mandatory item. The rules are `docs/agents/rules-tests.md`.
6. **No more than was asked.** You run no acceptance scenario, live model or demo and do not require them green,
   unless the task names them. An edit of `demos/` the task did not ask for, a re-shot `-example`, a live model run
   "to be sure" are mandatory items.
7. **Check it live, not by the report.** For the user paths the run changed, start a daemon in a temporary directory
   with its own `XDG_*` directories and walk the path by hand (CLI, API) as `docs/guide` says. Without a model.

## The score

`SCORE` is how far the task of the bead is done, 0 to 10, with one decimal at most: the share of what the task asked
that is done, corrected for quality. 9 and more only when the whole task is done and there is no mandatory item.

`VERDICT: complete`: the task is done and there is no mandatory item. Otherwise `VERDICT: continue`.

## The review file

`.loop/runs/<run>/review.md`, written anew each round, in Russian (the owner reads it). The first two lines are read
by a script and are exactly so; the headings are these:

```
SCORE: 7.5/10
VERDICT: continue

# Review of <bead id>, run <run id>

## Mandatory before going on
1. <what, where (file:line), why, how to check>, in the order of priority
## Desirable
## Checked
<the commands and their results: the tests run, the mutations, the live walk>
## Rule violations
```

"Mandatory before going on" holds what must be done before the task counts as done: violations of the owner's
decisions and of the rules, holes in what is called done, untruth in the report. Small things go to "Desirable".
What the owner must know or decide is also an entry appended to `.loop/JOURNAL.md`, in Russian, in the form
`CLAUDE.md` gives, with `run <run id>` in place of the cycle.

## Finishing

Stop everything you started in the background and wait for it to end. Then
`loop-manager finish pass --summary "SCORE <n>/10, VERDICT <verdict>"`: always `pass`, the gate reads the review.
`loop-manager finish fail --summary "<why>"` only when no review could be written at all.
`````

### `devloop/agents/reviewer/prompt.template.md`

`````markdown
Review the work on the bead `{{ bead.id }}`: {{ bead.title }}

<task>
{{ bead.description }}
</task>

The run is `{{ run.id }}`. You work in `{{ rig.path }}`; the coder's report is
`.loop/runs/{{ run.id }}/report.md`, and your review goes to `.loop/runs/{{ run.id }}/review.md`, written anew for
this round. The step `ci` passed on the current HEAD: {{ steps.ci.summary }}.

Review as your role says, then finish: `loop-manager finish pass --summary "SCORE <n>/10, VERDICT <verdict>"`.
`````

### `devloop/agents/expert/agent.toml`

`````toml
# A subagent of the coder: no step of a formula names it, so it has a role prompt and no prompt template.
description = "The expert of loop-manager: the plan or the design of a critical part (the core, the invariants, recovery, the harness protocols), a disputed design choice, a bug the coder failed to fix twice. Writes a document, changes no code."
harness = "claude"
model = "fable"
effort = "high"
`````

### `devloop/agents/expert/prompt.md`

`````markdown
You are the expert of loop-manager's own development (how the product works is in `docs/`, the rules of work in
`docs/agents/`). The coder calls you in three cases: to write or revise a plan, to design a critical part, to take
apart a bug it is stuck on. There is no operator: you ask no questions, you decide by the documentation and write
down what you assumed.

- The owner's decisions (`docs/agents/decisions.md`) are not reconsidered. A departure from an assumption
  (`docs/agents/assumptions.md`) needs its reason and an entry in `.loop/JOURNAL.md`.
- The result is a document in the file the coder named (under `.loop/runs/<run>/notes/`, or in `docs/` for a design
  that stays; `docs/` is in English, the files of `.loop/` in Russian). The answer is the path and the main points
  in 10 to 20 lines.
- You change and commit no code of the product. To diagnose you may run builds, tests and debugging commands.
- A design and a plan are measured by the budgets of `docs/agents/rules-code.md` and by "less code is better": the
  simplest variant that does what was asked. The "done when" of a piece of a plan is checkable, each piece fits
  100–150k tokens of context, and no acceptance scenario or demo is a condition unless the owner named it.
- Read the big documents (`docs/design/contracts.md`, `docs/design/config.md`) by section, and cut the output of
  commands.
- Before the last answer stop everything you started in the background and wait for it to end.
`````

### `devloop/agents/helper/agent.toml`

`````toml
# A subagent of the coder: no step of a formula names it, so it has a role prompt and no prompt template.
description = "A fast helper of the coder: a small edit by an exact description, a build, test or clippy run with a digest of the errors, a search over the code and the documents, a summary of a long file."
harness = "claude"
model = "sonnet"
effort = "medium"
`````

### `devloop/agents/helper/prompt.md`

`````markdown
You are the helper of loop-manager's own development. The coder calls you with one exact errand. There is no
operator: you ask no questions; when the errand is ambiguous, take the reasonable reading and say which.

- Do the errand and nothing beyond it. Do not commit.
- Cut the output of builds and tests (`| tail`, `| head`, `--message-format short`). The coder needs a digest: what
  failed, where (file:line), the text of the error; not the whole log.
- A long result goes to the file the coder named (under `.loop/runs/<run>/notes/`); the answer is the path and a
  digest of 5 to 15 lines.
- Before the last answer stop everything you started in the background and wait for it to end.
`````

### `devloop/agents/subcoder/agent.toml`

`````toml
# A subagent of the coder: no step of a formula names it, so it has a role prompt and no prompt template.
description = "A second coder for a separate piece of the implementation, on files the coder itself does not touch, in parallel with it. Does not commit."
harness = "claude"
model = "opus"
effort = "high"
`````

### `devloop/agents/subcoder/prompt.md`

`````markdown
You are the subcoder of loop-manager's own development. The coder calls you with an errand: what to implement, in
which files, by which documents of `docs/`, and how to check it. There is no operator: you ask no questions.

- The project's rules in `CLAUDE.md` and `docs/agents/rules-code.md` bind you: Rust, no stubs, a file of 400 lines
  at most, the latest versions of dependencies, a `MEASURE` marker at every measurement, and code, comments and
  documentation in English only.
- Change only the files of your errand: the coder works in the same tree beside you. An edit of another file is not
  made but described in the answer.
- Do not commit: the coder does. Before you return: `cargo fmt`, clippy and the tests of your piece are green, run
  targeted and with their output cut.
- An errand that does not fit your context: finish a whole part and describe the rest.
- The answer: what is done, the files changed, how it was checked, what is left, in 15 lines at most. Details, when
  needed, go to the file the coder named (under `.loop/runs/<run>/notes/`).
- Before the last answer stop everything you started in the background and wait for it to end.
`````

### `devloop/scripts/check-report.sh`

`````sh
#!/bin/sh
# The check of the step `code` (formulas/task.toml): the coder left its report, with the commit the run started
# from (the reviewer takes the diff from it), and nothing outside `.loop/` and the tracker is uncommitted, since the
# next step runs `mise run ci` on HEAD. Runs in the rig, the root of the repository, with the run's id in
# LOOP_MANAGER_RUN. Exit 0: passed; 1: not passed, and what it printed goes to the coder as a remark.
set -eu
report=".loop/runs/${LOOP_MANAGER_RUN:?}/report.md"
[ -f "$report" ] || { echo "check-report: no report: write $report"; exit 1; }
sed -n 1p "$report" | grep -Eq '^STATUS: (done|partial)$' || {
  echo "check-report: line 1 of $report must be 'STATUS: done' or 'STATUS: partial';" \
    "a blocked task ends with 'loop-manager finish fail'"
  exit 1
}
base=$(sed -n '3s/^BASE: //p' "$report")
git merge-base --is-ancestor "${base:-none}" HEAD 2>/dev/null || {
  echo "check-report: line 3 of $report must be 'BASE: <commit>': the HEAD the run started from, an ancestor of HEAD"
  exit 1
}
dirty=$(git status --porcelain -- . ':!.loop' ':!devloop/.beads')
[ -z "$dirty" ] || {
  echo "check-report: uncommitted changes outside .loop/: commit or undo them, the CI gate runs on HEAD"
  echo "$dirty" | head -n 20
  exit 1
}
`````

### `devloop/scripts/check-review.sh`

`````sh
#!/bin/sh
# The check of the step `review` (formulas/task.toml): the reviewer left this round's review with the two lines the
# gate reads, and changed nothing in the tree. Runs in the rig, the root of the repository, with the run's id in
# LOOP_MANAGER_RUN. Exit 0: passed; 1: not passed, and what it printed goes to the reviewer as a remark.
set -eu
dir=".loop/runs/${LOOP_MANAGER_RUN:?}"
review="$dir/review.md"
[ -f "$review" ] || { echo "check-review: no review: write $review"; exit 1; }
sed -n 1p "$review" | grep -Eq '^SCORE: (10|[0-9](\.[0-9])?)/10$' || {
  echo "check-review: line 1 of $review must be 'SCORE: <0 to 10, at most one decimal>/10', as 'SCORE: 8.5/10'"
  exit 1
}
sed -n 2p "$review" | grep -Eq '^VERDICT: (complete|continue)$' || {
  echo "check-review: line 2 of $review must be 'VERDICT: complete' or 'VERDICT: continue'"
  exit 1
}
# The review of an earlier round is older than the report the coder rewrote since.
[ -z "$(find "$dir/report.md" -newer "$review" 2>/dev/null)" ] || {
  echo "check-review: $review is older than the report: write the review of this round"
  exit 1
}
dirty=$(git status --porcelain -- . ':!.loop' ':!devloop/.beads')
[ -z "$dirty" ] || {
  echo "check-review: the tree has changes outside .loop/: a reviewer changes nothing, undo them (a mutation left in?)"
  echo "$dirty" | head -n 20
  exit 1
}
`````

### `devloop/scripts/gate.sh`

`````sh
#!/bin/sh
# The step `gate` (formulas/task.toml). Runs in the rig, the root of the repository, with the run's id in
# LOOP_MANAGER_RUN and its bead in LOOP_MANAGER_BEAD. It commits `.loop/` and the tracker and pushes, whatever the
# review says: a round sent back runs no step after this one. Then it judges `.loop/runs/<run>/review.md`.
# Exit 0: `SCORE` of 9 or more with `VERDICT: complete`. Exit 1: anything else; the run goes back to the coder with
# what this printed. Exit 75: the review passes and the push failed; the step is tried again.
set -eu
run=${LOOP_MANAGER_RUN:?}
review=".loop/runs/$run/review.md"
[ -f "$review" ] || { echo "gate: no review at $review"; exit 1; }
score=$(sed -n '1s/^SCORE: \(.*\)\/10$/\1/p' "$review")
verdict=$(sed -n '2s/^VERDICT: //p' "$review")

export GIT_TERMINAL_PROMPT=0   # a push that would ask for a password fails instead of waiting
git add -- .loop devloop/.beads
git diff --cached --quiet || git commit -q -m "devloop: review of ${LOOP_MANAGER_BEAD:-$run}, ${score:-no score}/10"
pushed=yes
git push -q || pushed=no

case "$verdict $score" in
  "complete 9" | "complete 9."[0-9] | "complete 10")
    [ "$pushed" = yes ] || { echo "gate: the review passes, but git push failed"; exit 75; }
    echo "gate: SCORE $score/10, VERDICT $verdict"
    ;;
  *)
    [ "$pushed" = yes ] || echo "gate: git push failed; the next round pushes again"
    echo "gate: SCORE ${score:-?}/10, VERDICT ${verdict:-?}: the bar is 9 with 'complete'. The review is $review:" \
      "its mandatory items are the first work of the next round"
    exit 1
    ;;
esac
`````

### `devloop/scripts/selftest.sh`

`````sh
#!/bin/sh
# Self-test of the scripts beside it, on sample files in a throwaway repository: each case names the exit code it
# expects. `mise run ci` runs it.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
export LOOP_MANAGER_RUN=run_1 LOOP_MANAGER_BEAD=lm-1
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null   # no hooks, signing or identity of the host
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.org
runs=.loop/runs/run_1

# A repository with a remote to push to, one commit, and the directories the scripts name.
fresh() {
  r=$(mktemp -d "$tmp/r.XXXXXX")
  git init -q --bare "$r/origin.git"
  git init -q "$r/work" && cd "$r/work"
  git remote add origin "$r/origin.git"
  mkdir -p $runs devloop/.beads src
  : >devloop/.beads/issues.jsonl
  echo one >src/a
  git add -A && git commit -q -m init && git push -q -u origin HEAD
}
expect() { # expect <exit code> <script> <label>
  got=0
  sh "$here/$2" >"$tmp/out" 2>&1 || got=$?
  [ "$got" = "$1" ] || { echo "selftest: $2: $3: expected exit $1, got $got"; cat "$tmp/out"; exit 1; }
}
report() { printf 'STATUS: %s\nBEAD: lm-1\nBASE: %s\n\n# A task\n' "$1" "${2:-$(git rev-parse HEAD)}" >$runs/report.md; }
review() { printf 'SCORE: %s\nVERDICT: %s\n\n# Review\n' "$1" "$2" >$runs/review.md; }

fresh; expect 1 check-report.sh "no report"
fresh; report done; expect 0 check-report.sh "done"
fresh; report partial; expect 0 check-report.sh "partial"
fresh; report blocked; expect 1 check-report.sh "blocked"
fresh; printf '# A task\nSTATUS: done\n' >$runs/report.md; expect 1 check-report.sh "STATUS not on line 1"
fresh; report done none; expect 1 check-report.sh "BASE is no commit"
fresh; printf 'STATUS: done\nBEAD: lm-1\n\n# A task\n' >$runs/report.md; expect 1 check-report.sh "no BASE line"
fresh; report done; echo two >src/a; expect 1 check-report.sh "uncommitted change"
fresh; report done; echo new >src/b; expect 1 check-report.sh "untracked file"
fresh; report done; echo x >devloop/.beads/issues.jsonl; expect 0 check-report.sh "only the tracker changed"

fresh; report done; expect 1 check-review.sh "no review"
fresh; report done; review 9/10 complete; expect 0 check-review.sh "9 complete"
fresh; report done; review 8.5/10 continue; expect 0 check-review.sh "8.5 continue"
fresh; report done; review 10/10 complete; expect 0 check-review.sh "10"
fresh; report done; review 11/10 complete; expect 1 check-review.sh "score over 10"
fresh; report done; review 8.55/10 continue; expect 1 check-review.sh "two decimals"
fresh; report done; review 9 complete; expect 1 check-review.sh "score without /10"
fresh; report done; review 9/10 done; expect 1 check-review.sh "unknown verdict"
fresh; review 9/10 complete; touch -t 200001010000 $runs/review.md; report done; expect 1 check-review.sh "review older than the report"
fresh; report done; review 9/10 complete; echo two >src/a; expect 1 check-review.sh "tree changed"

pushed() { [ "$(git rev-parse HEAD)" = "$(git -C ../origin.git rev-parse HEAD)" ] || { echo "selftest: gate.sh: $1: not pushed"; exit 1; }; }
committed() { [ -z "$(git status --porcelain)" ] || { echo "selftest: gate.sh: $1: .loop/ not committed"; exit 1; }; }
fresh; expect 1 gate.sh "no review"
fresh; report done; review 9/10 complete; expect 0 gate.sh "9 complete"; committed "9 complete"; pushed "9 complete"
fresh; report done; review 9.5/10 complete; expect 0 gate.sh "9.5 complete"
fresh; report done; review 10/10 complete; expect 0 gate.sh "10 complete"
fresh; report done; review 8.9/10 complete; expect 1 gate.sh "8.9"; committed "8.9"; pushed "8.9"
fresh; report done; review 9/10 continue; expect 1 gate.sh "9 continue"; pushed "9 continue"
fresh; report done; review 9/10 complete; echo x >devloop/.beads/issues.jsonl; expect 0 gate.sh "tracker"; committed "tracker"
fresh; report done; review 9/10 complete; git remote set-url origin "$r/gone.git"; expect 75 gate.sh "pass, push failed"
fresh; report done; review 7/10 continue; git remote set-url origin "$r/gone.git"; expect 1 gate.sh "fail, push failed"; committed "fail, push failed"
echo "devloop selftest: ok"
`````

### `devloop/README.md`

`````markdown
# devloop: loop-manager develops loop-manager

A loop-manager project whose rig is this repository. The owner files a task as a bead; a run of the formula `task`
has a coder do it, runs the CI gate, has a reviewer review it, and sends the work back to the coder until the review
scores 9 or more. Nothing here is part of the product: it is a pack and a config, read by `loop-manager` as any
other.

```
devloop/
  config.toml           the orchestrator config of the loop's own daemon
  loop-manager.toml     the project: the rig `repo` (the repository, `..`), the addon, the trigger
  agents/               coder and reviewer (steps of the formula); helper, subcoder, expert (subagents of the coder)
  formulas/task.toml    code -> ci -> review -> gate
  scripts/              the check scripts of `code` and `review`, the gate, and their self-test
  .beads/               the tracker of the tasks (br), committed with the code
```

## A run

| Step | What it does | When it does not pass |
|---|---|---|
| `code` | the coder does the task, commits, writes `.loop/runs/<run>/report.md`; `scripts/check-report.sh` wants the report's first three lines and a tree with nothing uncommitted outside `.loop/` | the coder gets the script's output and goes on, up to 3 times |
| `ci` | `mise run ci` on HEAD | back to `code` with the tail of the output |
| `review` | the reviewer writes `.loop/runs/<run>/review.md`; `scripts/check-review.sh` wants its `SCORE:` and `VERDICT:` lines and an unchanged tree | the reviewer gets the script's output and goes on, up to 3 times |
| `gate` | `scripts/gate.sh` commits `.loop/` and `devloop/.beads/`, pushes, and passes on `SCORE` of 9 or more with `VERDICT: complete` | back to `code`; the review is the remark |

A run has 4 rounds (`rounds` in `formulas/task.toml`); a red `ci` spends one, and so does a review under the bar. A
run that passes closes its bead. A run that fails releases it: the bead is ready again and gets a new run with new
sessions, which read the reports and reviews the earlier runs left. After 3 failed runs (`bead_failures`) loop-manager
closes the gate `bead:devloop/<id>`, and nothing more starts on that bead until you open it
(`loop-manager gate list`, `loop-manager gate open <key>`).

One run at a time works in the repository (`max_writers = 1` of the rig): the others wait in `queued`. The session
of the coder and of the reviewer is compacted at a window of 300000 tokens, and the run fails at the fifth compaction
of one session (the addon `addons/context-budget`, attached in the two `agent.toml`).

## Before the first start

- `loop-manager`, `claude`, `br`, `jq`, `git` and `mise` are on the `PATH` of the shell that starts the daemon, and
  `loop-manager doctor --config devloop/config.toml` is content.
- `git push` works from that shell without a question: see "The push".
- The loop this pack replaces is switched off: see "Moving over". Until then its hooks in `.claude/settings.json`
  run inside every claude session that works in this repository, the sessions of this pack included.

## Start it

```sh
loop-manager daemon --config devloop/config.toml
```

The daemon keeps its state in the XDG state directory and listens on its default socket. To keep it apart from
another daemon of yours, give it its own `XDG_STATE_HOME` and `LOOP_MANAGER_SOCKET`, as `demos/*/run.sh` do, and
export the same `LOOP_MANAGER_SOCKET` in the shell you run the commands below from.

## File a task

A ready bead with the label `auto` starts a run; a bead without it starts nothing.

```sh
cd devloop
br create "Short title of the task" -l auto -d "What to do, by which documents, and when it is done"
```

Write the description as a plan package is written: the documents, the result, the checkable "done when". The size
of a task is what one coder session does, with its subagents. A task that waits for another is filed with a
dependency (`br dep add <task> <what it waits for>`) and becomes ready when that one is closed.

By hand, without the label: `loop-manager run start devloop repo task --input task=<bead id>`, or
`--input 'task=<the text of the task>'`, which files the bead.

## Watch it

```sh
loop-manager run list                 # the runs, newest first: state, step, formula
loop-manager run show <run>           # one run: its round, the outcomes of its steps, why it failed
loop-manager run watch <run>          # one run live, with the messages of its session
loop-manager events                   # every event, live
loop-manager trigger list             # `task-ready` and whether it is on
```

What a run leaves for you is in the repository, committed and pushed by the gate after every reviewed round:
`.loop/runs/<run>/report.md` and `review.md` (the history of a run's rounds is the git history of these files),
`notes/` beside them, and `.loop/JOURNAL.md`, where the agents write what you need to know or decide. A coder that
cannot go on without you writes the blocker there and fails its run.

## The push

The exec steps and the check scripts get the whole environment of the daemon (`[exec] inherit_env = true` in
`loop-manager.toml`), not `PATH`, `HOME` and `LANG` alone, so the `git push` of the gate authenticates as your shell
does: through the ssh agent (`SSH_AUTH_SOCK`), a token (`GH_TOKEN`, `GITHUB_TOKEN`) or a credential helper. Check
it in the shell that will start the daemon: `git push --dry-run`. A key with a passphrase and no agent cannot work:
nothing can ask for it, and the gate fails instead of waiting. When the review passes and the push does not, the
step is tried again three times, 30 s apart, and then the run fails; the commit stays local and the next push takes
it along.

## Who may change this directory

Only you. The daemon holds the config in memory until `loop-manager config reload`, but it runs the scripts under
`scripts/` from the working tree at the moment a step needs them: a coder that edits `scripts/gate.sh` changes the
check of its own round. The reviewer therefore reports any change under `devloop/`, other than `.beads/`, as a
mandatory item that waits for you, and a run with such a change does not pass by itself.

If that is not enough, pin the pack. `packs` in `loop-manager.toml` takes `{ git = "<url>", rev = "<commit or
tag>" }`, where the url is anything `git clone` takes, an absolute path of a local repository or `file://` included,
and loop-manager then reads the pack from a checkout in its cache that no agent works in. A pack from git is a
repository with `pack.toml`, `agents/` and `formulas/` at its root (there is no key for a subdirectory), so the pin
needs a repository of its own for `agents/`, `formulas/` and `scripts/`, not this one. The check scripts are found
from the formula's directory and so come from the pinned checkout; the command of the step `gate` is found from the
rig, so in a pinned pack make it a check script too (`step_check_script_path` on a step whose command is `true`).

## Moving over

The loop this pack replaces lives in `.claude/` and `scripts/loop/`. The two do not work side by side: its hooks
would run in the sessions of this pack, and both would commit to `.loop/`. To move over, between two cycles of the
old loop:

1. Remove `.claude/agents/` and `.claude/hooks/`, and the `hooks` table from `.claude/settings.json` (its `env` and
   `enabledPlugins` stay: the sessions of this pack inherit them).
2. Remove `scripts/loop/`.
3. In `CLAUDE.md` and `docs/agents/loop.md`, replace what describes cycles (`loopctl`, `scripts/loop/checks`,
   `.loop/cycles/`, `.loop/state.json`, `.loop/handoffs/`, the 300k and 420k thresholds, "the harness of the loop")
   with a pointer to this file; the roles of this pack tell their agents to skip those parts until then.
4. File what is left of `.loop/PLAN.md` as beads.

After a fresh clone br has no database yet: `cd devloop && br init --prefix dev` makes it, and br fills it from
`.beads/issues.jsonl`.
`````

## 4. Правки вне `devloop/`

1. `mise.toml`, задача `[tasks.ci]`, в конец массива `run` (после строки про `CARGO_BIN_EXE_loop-manager`):

   ```toml
     # The pack that develops loop-manager with loop-manager (devloop/README.md): its scripts on sample files, and its
     # config loads.
     "devloop/scripts/selftest.sh",
     "cargo run -q --locked -p loop-manager -- config check --config devloop/config.toml",
   ```

2. `docs/guide/development.md`, строка таблицы про `mise run ci` (строка ~42): перечень того, что гоняет гейт,
   теперь расходится с кодом. В конец первой ячейки перечня (перед ` | **the gate of a change**`) дописать:
   ``, and the pack in `devloop/` ([its README](../../devloop/README.md)): the self-test of its scripts and a load of
   its config``.
3. `docs/README.md`, таблица-карта (рядом со строкой про `../demos/README.md`), одна строка:
   `| [../devloop/README.md](../devloop/README.md) | the pack that develops loop-manager with loop-manager |`.

Больше ничего вне `devloop/` не трогается. `docs/design/config.md` не меняется: новых ключей нет.

## 5. Как это устроено (для проверки смысла, не для переписывания)

### Формула

- `code` (ai, coder, `max_attempts = 3`, check `../scripts/check-report.sh`) → `ci` (exec `mise run ci`,
  `back_to = "code"`) → `review` (ai, reviewer, `max_attempts = 3`, check `../scripts/check-review.sh`) → `gate`
  (exec `sh devloop/scripts/gate.sh`, `back_to = "code"`, `timeout = "10m"`).
- `step_check_script_path` считается от каталога файла формулы (`devloop/formulas/`), отсюда `../scripts/…`;
  скрипт запускается в workdir прогона (корень репозитория) с окружением exec-шага (`LOOP_MANAGER_RUN`,
  `LOOP_MANAGER_BEAD`, …) и только после того, как агент сказал `pass`. Выход 0 — прошёл; другой код — «не прошёл»:
  при `max_attempts = 3` шаг повторяется в той же сессии, вывод скрипта приходит агенту замечанием (у coder — через
  `{{ remarks }}`, у reviewer дописывается к промпту). Код 75 скрипты проверки не возвращают (он финальный).
- Команда exec-шага запускается в workdir рига, подстановок в ней нет — поэтому `sh devloop/scripts/gate.sh`
  (путь от корня репозитория; через `sh`, чтобы не зависеть от того, как spawn разрешает относительный путь).
- Ряд проверок (`crates/loop-manager-core/src/run/finish.rs:127-137`): после красного `ci` следующий шаг (`review`)
  не проверка, ряд кончается, прогон сразу уходит на `code`; `review` и `gate` в этом раунде не идут — поэтому
  коммит `.loop/` и push сидят внутри `gate.sh` и делаются при любом вердикте.
- Замечания нового раунда: у exec-проверки — имя, `exit <код>` и хвост вывода (4 КиБ). `gate.sh` печатает одну
  строку с оценкой и путём к ревью; сам текст ревью кодер читает из файла.
- Таймауты ai-шагов и `ci` — умолчания (4h и 45m).
- `STATUS: blocked` в отчёте не бывает: заблокированный кодер пишет БЛОКЕР в журнал и делает
  `loop-manager finish fail` (check-скрипт после `fail` не запускается, прогон падает, счётчик bead'а +1).

### Скрипты

| Скрипт | Вход | Выход | Что делает |
|---|---|---|---|
| `check-report.sh` | cwd — корень репозитория; `LOOP_MANAGER_RUN` | 0 / 1 (+ строка причины в stdout) | `.loop/runs/$RUN/report.md` есть; строка 1 — `STATUS: done\|partial`; строка 3 — `BASE: <коммит>`, предок HEAD; вне `.loop/` и `devloop/.beads` нет незакоммиченного |
| `check-review.sh` | то же | 0 / 1 | `review.md` есть; строка 1 — `SCORE: N[.N]/10` (0–10); строка 2 — `VERDICT: complete\|continue`; ревью не старше отчёта (ловит ревью прошлого раунда); дерево чистое (не осталась мутация) |
| `gate.sh` | то же + `LOOP_MANAGER_BEAD` | 0 pass; 1 fail; 75 — ревью проходит, но push упал (попытка шага повторяется политикой: `unknown` → resume через 30 с, до 3 раз) | `git add .loop devloop/.beads`, коммит `devloop: review of <bead>, <score>/10` если есть что, `git push` (`GIT_TERMINAL_PROMPT=0`); pass при `VERDICT: complete` и `SCORE` 9, 9.x или 10 |
| `selftest.sh` | без аргументов | 0 / 1 | для каждого случая делает во временном каталоге git-репозиторий с bare-remote, кладёт образцы `report.md`/`review.md`, запускает скрипт и сверяет код выхода; у gate ещё проверяет, что `.loop/` закоммичен и запушен. 29 случаев, ~2 с. `GIT_CONFIG_GLOBAL=/dev/null` — чтобы глобальные хуки и подпись хоста не мешали |

Самопроверка проверена мутациями (10 из 10 убиты): планка 9, код 75, push, `devloop/.beads` в коммите, `blocked`,
исключение трекера из «грязного дерева», вердикт, свежесть ревью, номер строки `SCORE`, проверка `BASE`.

### Роли: что перенесено, что убрано, что добавлено

- Перенесено по смыслу из `.claude/agents/*.md` и `docs/agents/loop.md`: «оператора нет», журнал, порядок работы
  кодера, таблица субагентов и правила брифа, правила реализации и коммитов, формат отчёта; у ревьюера — «что
  проверить», «строгость» (мутации, ослабление документации, самоуправство, флейки, «не больше, чем просили», живая
  проверка), оценка, формат ревью; expert/helper/subcoder — почти дословно.
- Убрано: `loopctl`, `state.json`, `handoffs`, номера циклов, мастер/overseer, пороги 300k/420k, `scripts/loop/checks`
  и `checks.md`, «пуш делает цикл», раздел «Планирование, если плана нет», строка `BLOCKERS:` ревью, правила `br`
  старого лупа. В каждой роли есть абзац: где `CLAUDE.md` и `docs/agents/loop.md` говорят про циклы и харнесс — это
  прежний луп, действует роль (нужно, пока владелец не поправит эти файлы; `CLAUDE.md` claude грузит сам).
- Добавлено: шаги формулы и что происходит при возврате; каталог прогона; строка `BASE:` в отчёте (HEAD на старте
  прогона — от неё ревьюер берёт дифф; базы цикла больше никто не даёт); поиск отчётов прежних прогонов по
  `BEAD:`; «не гоняй полный `mise run ci` — его гонит шаг `ci`»; завершение через `loop-manager finish`; у кодера —
  `devloop/` не менять, bead'ам не ставить метку `auto`, пункт «ждёт владельца» не его; у ревьюера — **любая правка
  `devloop/` кроме `devloop/.beads/` = обязательный пункт «ждёт владельца»** (`.beads` исключён: его меняет сам
  демон при claim/close, иначе пункт был бы в каждом ревью), гейт не перезапускать (шаг `ci` только что прошёл на
  этом HEAD), оценка — по задаче bead'а, а не по плану.
- Язык: файлы пака — только английский (заголовки отчёта и ревью тоже); роль велит писать отчёт, ревью и журнал
  по-русски. Машинные строки (`STATUS:`, `BEAD:`, `BASE:`, `SCORE:`, `VERDICT:`) — ASCII.
- Субагенты: `subagents = ["helper", "subcoder", "expert"]` в `agents/coder/agent.toml`; цели обязаны иметь
  `description` (есть в их `agent.toml`). Имена типов в сессии — **с именем пака**: `devloop.helper`,
  `devloop.subcoder`, `devloop.expert` (`crates/loop-manager-contract/src/config/subagents.rs`, виден в explain) —
  так они и названы в роли кодера. У reviewer списка нет и `description` нет — звать ему некого (встроенные типы
  claude отключены). `env` задан один раз в `[settings]` проекта: у цели субагента ключ `env` с другим значением —
  ошибка загрузки, а общий слой даёт всем одно и то же.

## 6. Проверка (сделано на черновике; subcoder повторяет на реальном дереве)

`config check --config devloop/config.toml` — выход 0, без вывода.

`config explain --config devloop/config.toml --project devloop --agent coder | grep -v '^system_prompt'`
(пути заменены на `<repo>` и `~`):

```
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

`config explain … --formula task --rig repo` (без `system_prompt`, `env.`, `hooks.`, `addons.`, `native.`):

```
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

Пробный демон без модели (во временном каталоге, та же пара `config.toml` + `loop-manager.toml`, формула `task`
заменена на один exec-шаг): bead `lm-…` с меткой `auto` → `trigger.fired`, прогон на риге `repo`, cwd — корень
репозитория, `BD_ISSUE_PREFIX=lm`, переменная оболочки демона видна в шаге (`inherit_env`); второй такой bead ждал
в `queued`, пока шёл первый (`max_writers = 1`); bead с префиксом базы `dev-…` и меткой `auto` тоже ушёл в прогон на
риге `repo`; bead без метки прогон не запустил; после `passed` bead'ы закрыты. Демон в каталоге проекта создаёт
`.bv/` — отсюда `devloop/.gitignore`. `br init --prefix dev` поверх `.beads` без файла базы (свежий клон) отрабатывает.

Не проверено (нужна живая модель, пакет это запрещает): рендер шаблонов на настоящем bead'е, доставка субагентов
и хуков аддона в claude, сам `git push` на GitHub. Загрузчик шаблоны проверил (пути `{{ … }}` существуют).

Готово, когда (из пакета): `config check` зелёный и стоит строкой в `mise run ci`; explain кодера показывает модель,
субагентов, аддон и `env` (вывод в отчёт); `devloop/scripts/selftest.sh` зелёный; `mise run ci` зелёный.

## 7. Источник пака `{ git = …, rev = … }`

`git` принимает всё, что принимает `git clone`: строка идёт в `git clone -- <url>` и `git ls-remote` как есть
(`crates/loop-manager/src/git_cache.rs:111-125`), то есть абсолютный локальный путь и `file://` работают (относительный
путь считался бы от cwd процесса — не использовать); `rev` — тег или полный id коммита. **Но** пак из git — это
репозиторий с `pack.toml`, `agents/`, `formulas/` в корне, ключа подкаталога у пака нет (`dir` есть только у
аддона), поэтому закрепить пак «на этот же репозиторий» при раскладке `devloop/` нельзя: нужен либо отдельный
репозиторий под пак, либо новый ключ (код продукта — вне пакета). Второе ограничение: из закреплённого checkout'а
берутся только check-скрипты (путь от каталога формулы), команда шага `gate` ищется от рига, из живого дерева; в
закреплённом паке её надо сделать check-скриптом шага с командой `true`. Оба факта записаны в README («Who may
change this directory») и в журнал. Вживую клонирование по локальному пути не запускалось — вывод по коду.

## 8. Риски и чего не делать

- **Не запускать пак вживую** и не заводить в `devloop/.beads` bead'ы с меткой `auto`.
- **Хуки старого лупа.** `.claude/settings.json` репозитория вешает `loop_guard.py` на `PreToolUse`, `PostToolUse`,
  `SubagentStart/Stop`, `PreCompact`, `Stop`, `SessionStart`; claude грузит настройки проекта в любой сессии с cwd в
  репозитории, значит до переезда эти хуки сработают и в сессиях пака (и `.claude/agents/` даст сессии старые типы
  агентов). Пак нельзя запускать рядом со старым лупом; в README это стоит условием первого старта. Пакет
  `.claude/` не трогает.
- **Модель и окно.** `opus`/`fable` — короткие имена, claude разрешает их сам. Если у владельца `opus` разрешается в
  вариант с окном меньше 300k, harness сожмёт раньше, по своему пределу; счётчик сжатий при этом работает, но
  бюджет прогона выйдет меньше расчётного. Вживую не проверялось.
- **Незакоммиченный `devloop/.beads/issues.jsonl` после прогона.** Bead закрывается демоном после `gate`, так что
  запись о закрытии попадает в коммит следующего прогона. Скрипты проверки это дерево «грязным» не считают.
- **`inherit_env`** отдаёт exec-шагам и check-скриптам всё окружение демона, включая токены. Это свои скрипты на
  машине владельца; вывод шага попадает в журнал событий — скрипты переменные не печатают.
- **Пункт «ждёт владельца»** не закрывается кодером: прогон с правкой `devloop/` не пройдёт сам — раунды кончатся,
  либо кодер завершит `fail` с блокером; после трёх упавших прогонов закроется gate bead'а. Так и задумано.
- Никакого кода продукта и новых ключей конфига; `.claude/`, `scripts/loop/`, `CLAUDE.md`, `docs/agents/loop.md`,
  `demos/` не трогать; рекурсивное удаление через `rm` не использовать (хук отклонит). Новый код продукта для пакета
  **не нужен**.
- Содержимое explain в отчёт вставлять с фильтром; вне `.loop/` домашних путей не писать (гейт ищет их везде, кроме
  `.loop`).

## 9. Вопросы владельцу (записаны в `.loop/JOURNAL.md`, до ответа действует выбранное)

1. Закрепление пака на этот же репозиторий невозможно без ключа подкаталога для пака или отдельного репозитория
   (раздел 7). Выбрано: оставить локальный пак, описать в README.
2. Метка `auto` на триггере — добавление сверх текста пакета. Альтернатива: запускать любой готовый bead.
3. `rounds = 4`, окно 300k и 4 сжатия, `max_writers = 1`, `inherit_env` на весь проект — предложения, меняются одной
   строкой каждая.

# Цикл 93 — coder
STATUS: done

## Взято в работу
WP-77 (M, критичный): корень проекта (`[root]` name/path) и область агента (`scope`). Начат с плана expert.
## Сделано (коммиты)
- План expert: `.loop/cycles/93/notes/wp77-plan.md`.
- `d16a818` — `Project.root` → `Project.dir` (каталог `aae.toml`).
- `e099bad` — корень как место: `[root] name/path/vars`, `config/places.rs` (`Project::place/places`), риг с именем
  или путём корня — ошибка; поиск мест в рендере, очереди, «объясни», reload, doctor; проектный триггер без `rig` — в
  корне; демо без `[rigs.root]`; тесты ядра `tests/config_root.rs`.
- `51de260` — `scope = "rig"|"root"` в `agent.toml`, `EffectiveConfig.scope`, `session_dir` (сессия агента `root` — в
  корне), агент `root` без слоя/правок/варианта рига.
- `9d0b788` — демо: planner/qa `scope = "root"`, `run.sh` без рига, промпт plan.md.
- `c9c19a2` — worktree в каталоге без git — `deny` с причиной (subcoder), тест testkit.
- `c9aa1a8` — «a start without a rig works in the root» — `rig_of_bead`: без бида или без метки `rig:` — корень (и дети
  `drain`); тесты testkit `tests/root.rs` (5: без рига/без git, корень с именем и путём, бид корня, агент `root` на
  прогоне рига, ворота корня), правки `beads.rs`, `drain.rs`, `drain_steps.rs`, `cli.rs` (subcoder); драйвер testkit
  отдаёт каталог сессии.
- `bf89461` — фронт (subcoder): `/v1/projects` = `{name, dir, root, rigs}`, explain формулы без рига — на корне,
  тексты `run start`/`POST /v1/runs`/MCP, OpenAPI.
- `5f72c97` — доки: `config.md` (`[root]`, `scope`, drain, шаблон), `contracts.md` 6.1, `tracker-br.md`,
  `frontends.md`, `mcp.md`, гайды `first-project.md` (проект без ригов) и `first-formula.md`.
- `8c70452` — `acceptance-ci`: строка 27 расширена (11 тестов), новая строка 32 (5 тестов), `"32"` в `ROWS`.
- `7a3b391` — READINESS: «Config assembly» — критерии 12, 13, 26, 27, 32, оценка 8.
- Журнал: РЕШЕНИЕ по [Д] WP-77 (6 пунктов).
- Проверки: `scripts/loop/checks 93` на `7a3b391` — pass: ci 69 с, nextest 479/479, acceptance 30 pass / 0 fail /
  9 not run (строки 27 и 32 — pass). `.loop/PLAN.md`: WP-77 — done.
## Не получилось и почему
- Часть критерия 32 про префиксы ригов — пакет WP-78 (по плану). Отказ «bead … is not in the ready list: name the
  rig» оставлен как есть (WP-78 заменит метку префиксом).
- codex/claude в каталоге без git вживую не проверены (адаптеры git не проверяют; живой прогон — WP-84).
## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующий пакет — WP-78 (критичный).
## Замечания предыдущего ревью: что закрыто, что нет
Ревью цикла 92 нет (ревьюер не оставил SCORE/VERDICT, см. журнал 10:58:01).
## Контекст на момент отчёта: ~150k

# Цикл 68 — coder
STATUS: done

## Взято в работу
- WP-59 (M) — харнесс вместо провайдера: код, API/OpenAPI, тесты, docs, демо вживую.
- WP-58 (S) — лексическая нормализация путей конфига, обход в `demos/run.sh` убран.
- OWNER-RULES §7 (против флейков): `biased;` во всех `select!` ядра и проверка в CI.
- Желательное ревью 67: п. 2 (документы), п. 3 (фраза о SIGKILL хуков), п. 4 (`process.rs` 399 строк — разбит).
- WP-60 (S) — требования спеки, которых нет в `docs/` (через subcoder, проверено и закоммичено мной).
- WP-61 (S) — `doctor` и вход codex через модельного провайдера хоста.
- WP-57 — только дизайн у эксперта (пакет оказался не S, см. журнал).

## Сделано (коммиты)
- `d5d927b` WP-59: нет `provider` ни в одном слое (неизвестное поле), нет слоя `derived` и вывода харнесса,
  у `Adapter` нет `providers`; `launch` — сильнейший слой с `harness`, иначе «no layer sets `harness`» (проверка
  «харнесс не в оркестраторе» — уже при загрузке каждого слоя, дубль в `compose` убран). Ключ модели —
  `model:<harness>/<раскрытое имя>` только из снимка конфига (`run/gates.rs` `keys`: снимок, а до снимка колонка
  `runs.model`); модель из сбоя ключ больше не добавляет (`run/machine.rs`), поле `provider_model` осталось данными.
  `GET /v1/agents` без `provider`; OpenAPI перегенерирован. Тесты: `config_launch.rs`, `config.rs`, `model_gate.rs`
  (сбой с другой моделью ключа не добавляет), `cli.rs`, `role_prompt.rs`, `aae-plugins` (ключи `model:claude/…`).
  Демо: вариант `codex-coder` — `harness = "codex"`, агенты пака — `harness = "claude"`, README демо.
- `b24c3d0` три `select!` (`process.rs`, `run/service_proc.rs` ×2) с `biased;` и комментарием порядка (остановка →
  исход процесса → таймер/проверка здоровья); `scripts/select-biased.sh` в `mise run ci`; юнит-тесты `process.rs`
  вынесены в `process/tests.rs` (248 строк вместо 401); перенос шапки `aae/src/doctor.rs`.
- `36bd630` docs: `config.md` «Harness and model», contracts 7 и 6.4 (SIGKILL хука без ожидания смерти; перенос
  длинной строки), `adapters.md`, `invariants.md` Recovery (sweep маркеров хуков), `install.md` (`GET /v1/doctor`),
  `first-formula.md`, `READINESS.md`.
- `5640682` WP-58: `config/items.rs` `resolve` убирает `.` и `..` лексически, симлинки не раскрывает; тест
  `a_rig_path_has_no_dot_and_no_dot_dot` (`.`, `./x/./y/..`, `../x`).
- `90aca9d` демо вживую: `snake-claude` passed 99 с, `snake-mixed` (codex `luna`) passed 119 с; снимки; `run.sh` без
  `gsub`.

- `67e8c07` WP-61: `aae-codex` `login`: с провайдером модели в конфиге хоста проверяется его `env_key`; не задана
  переменная — warning с её именем; у провайдера нет `env_key` — warning «not checked», а не «нет auth.json».
  Тест — `aae/tests/doctor.rs` (два новых случая); `install.md` (про «Not logged in» у `codex login status`),
  `config.md` (известные ограничения).
- `e05fcce` WP-60: `development.md` (граница ядра и порядок `select!` в CI, публичный репозиторий), `frontends.md`
  (без доменной логики), `adapters.md` (таблица сокращений сверяется с харнессом, отказ — находка), `first-formula.md`
  (пак не знает задачи), `config.md` (мост MCP вне ядра); спека: §12 `explain --agent|--formula --rig`, критерий 20
  (пак и конфиги демо грузятся `aae config check`). §7.5 не правил: идентификаторов версий в спеке нет.
- `56f72de` READINESS: оценка на `67e8c07` (маркер не старше последней правки `crates/`), оба демо прошли.

Точечно: `aae-core`, `aae-testkit`, `aae-plugins`, `aae-front`, `aae`, `aae-claude`, `aae-codex` — 407/407; clippy
workspace чистый.

## Не получилось и почему
- WP-57 не взят: создание прогона синхронно и требует риг, а метки бида ядро знает только асинхронно; в интерфейсе
  трекера нет «показать бид». Эксперт дал дизайн: `.loop/cycles/68/notes/wp57-design.md` — риг из `ready_beads` в
  транзакции создания, `Create.rig: Option`, CLI `<PROJECT> [RIG] <FORMULA>`; граница [Д]: метка известна только у
  бида из готового списка, иначе отказ «name the rig». Оценка ~60–70k — следующему циклу.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Ничего не начато сверх сделанного. Следующий шаг — WP-57 по дизайну эксперта, затем WP-62, WP-63, WP-64.

Проверки: `checks.md` — HEAD `56f72de`, RESULT pass: ci 61 с (nextest 416/416), acceptance-ci 21 pass / 0 fail /
9 not run. Прогнана копия `scripts/loop/checks` из `mktemp -d`: оригинал отказывает из-за незакоммиченных правок
харнесса владельца (`.claude/agents/reviewer.md`, `scripts/loop/loopctl`); отличия копии — `cd` в репозиторий и
исключение этих двух путей (журнал 13:20).

## Замечания предыдущего ревью: что закрыто, что нет
Обязательных не было. Желательные:
1. Тест хука `run.finishing` после рестарта — уже был закрыт коммитом владельца `aaf1f51`
   (`a_restart_kills_the_finishing_hook_the_dead_daemon_left`).
2. Документы: invariants Recovery, install.md `GET /v1/doctor`, длинные строки — закрыто (`36bd630`, `b24c3d0`).
   OpenAPI `/v1/doctor`: 500 в документе уже нет, только 200 и общий 401.
3. Фраза о SIGKILL хука без ожидания — contracts 6.4 (`36bd630`).
4. `process.rs` разбит (`b24c3d0`); `aae/tests/api.rs` 398, старый `lock`-файл — не трогал.
5. Вопрос владельцу по §11.3 — без изменений.

## Контекст на момент отчёта: ~155k

# WP-H — настройки запуска агента: провайдер, вывод харнесса, сокращения моделей, слои пака и формулы, варианты
STATUS: done (кроме правки чужого `queue.rs` — готовый патч ниже)

## Сделано

Коммиты:
- `0822316` Launch settings: provider and harness per layer, short model names, pack and formula layers, agent variants
  (шаги 1–4 вместе: они переплетены в одной сборке конфига).
- `5adce44` The model gate key of a run is its config's, model:<provider>/<model> (шаг 5).
- `4471158` docs: launch settings, agent variants, adapter providers and the model gate key (шаг 6).

1. **provider/harness — послойные скаляры.** `Settings` получил `harness`, `provider` (любой слой). `Adapter` получил
   `providers` (свой первым) и `models` (сокращения). Ядро знает таблицу через `Config::adapters`, корень передаёт её
   в `config::load_for(file, ADAPTERS)`; `config::load(file)` = сборка без адаптеров (testkit со scripted-харнессом).
   Вывод — `compose.rs::launch`: только provider → единственный адаптер; только harness → его первый провайдер;
   ничего → ошибка; провайдер без харнесса/несколько харнессов → ошибка; выведенный харнесс обязан быть в
   `[harness.*]`. `scripts/core-boundary.sh` проходит. Тесты: `tests/config_launch.rs`
   (`harness_and_provider_follow_from_each_other`, `an_override_changes_the_harness`,
   `launch_settings_that_do_not_go_together_are_load_errors`).
2. **Сокращения.** codex: `astra/luna/terra/sol` по таблице владельца; claude: пустая таблица — CLI сам раскрывает
   (`--model`: «an alias for the latest model (e.g. 'fable', 'opus', or 'sonnet')»). Раскрытие — при сборке;
   `EffectiveConfig.model_given` хранит заданное имя, explain: `model = "gpt-6-luna"  # rig override, given as "luna"`.
   Живой прогон: 1 × `claude -p --model haiku` → `claude-haiku-5-5`, $0.0017 (журнал). codex `model/list` (без вызова
   модели): есть `gpt-6-astra`, `gpt-6-luna`, `gpt-6.1-terra`, `gpt-6.1-sol` — **имён владельца `gpt-6-1-terra`,
   `gpt-6-1-sol` нет** (через точку). Таблица оставлена по словам владельца, находка и вопрос — в журнале. Точную ошибку
   codex на `gpt-6-1-terra` получить не удалось: живой вызов `codex exec -m gpt-6-1-terra` запрещён песочницей
   (classifier). Живой прогон на `luna` не делал: `gpt-6-luna` есть в `model/list`.
3. **Слои пака и формулы.** `pack.toml [settings]` и ключи `harness|provider|model|effort` в `[step]` prompt-шага
   (`Launch`, `deny_unknown_fields`: прочие ключи — ошибка; в exec-шаге — «unknown field»). Порядок в
   `Config::for_run`: оркестратор → проект → риг → плагины → агент → пак → формула → вариант → оверрайды
   оркестратора/проекта/рига. Слои в explain: `pack <имя>`, `formula <pack>.<name>`, `variant <имя>`, `derived`.
   Тест `the_pack_and_the_formula_step_are_layers`.
4. **Варианты.** `[agents.<name>]` (оркестратор и проект; вариант проекта заменяет одноимённый вариант оркестратора),
   `base`, любые ключи агента кроме `prompt`, свой `max_runs` (иначе базового). `[rigs.<r>] agents = { ref = "var" }`.
   Роль (промпт, skills) — базового агента; оверрайды — только названные именем варианта. Ошибки: `base` не найден,
   `base` — вариант, отображение на несуществующий вариант, имя варианта = агент пака. Тесты
   `a_rig_runs_a_variant_in_place_of_the_agent`, `variant_errors`. `Config::max_runs(project, agent)` — лимит варианта.
5. **Ключ ворот модели.** `gates.rs::keys` берёт `model:<provider>/<model>` из снимка конфига (SQL json_extract) и
   добавляет выученный из сбоя (если другой); `machine.rs` пишет ключ сбоя с провайдером из снимка. Итог без чужих
   файлов: новый прогон на закрытой модели **паркуется до первой попытки** (не стартует ни одной). Тесты
   `crates/aae-testkit/tests/model_gate.rs`: оба красные без правки gates/machine (проверено обратным патчем).
   Чтобы прогон ждал именно в `queued` — нужен патч `queue.rs` (ниже).
6. **Документы.** `config.md`: таблица ключей, слои 1–10, новый раздел «Harness, provider and model», раздел
   «Agent variants», примеры pack/project/formula, `ver=prior-a86d05bd7`. `adapters.md`: `providers`/`models` и абзац
   про будущую `[provider.<p>] harness`. `contracts.md` §7: ключ модели известен из конфига до первой попытки.

Попутно (необходимые места сборки конфига): `run/render.rs` (снимок прогона — через `for_run`, проверка агента
варианта), `run/reload.rs` (отпечатки — по формулам × ригам через `for_run`), `aae/src/doctor.rs` (агенты отдельно и
прогоны формул, варианты по имени), `aae/src/harness.rs` (`ADAPTERS` → `load_for`).

## Не сделано / нужно от мастера

1. **Патч `crates/aae-core/src/run/queue.rs` (зона WP-I; правка чужого файла запрещена — не применял):**
   `.loop/audit2/work/WP-H-notes/queue.patch` (`git apply` проходит на HEAD `4471158`). Делает: агент прогона =
   `for_run` (вариант под своим именем; без этого прогон варианта упадёт в `render` с «now runs agent `fast`»),
   ключ модели пишется при создании (`learn_keys`) → закрытые ворота держат прогон в `queued`; лимит `max_runs` через
   `Config::max_runs` (варианты). В патче два теста в `model_gate.rs` (`a_closed_model_gate_keeps_a_new_run_queued`,
   `a_rig_runs_the_variant_under_its_name`). Проверено в отдельном worktree: aae-core + aae-testkit 268/268, clippy и
   rustfmt чисто. **Без патча варианты в живых прогонах не работают.**
2. **Front (зона WP-I):** `GET /v1/agents` теперь отдаёт `harness: null`, если `agent.toml` харнесс не называет —
   `replies.rs` `Agent.harness: String` → `Option<String>` и перегенерировать `docs/api/openapi.json`. Правку
   `api/config.rs` на вывод харнесса через `effective` песочница запретила (чужая зона).
3. **explain с формулой и вариантом:** `aae config explain` / `GET /v1/config/explain` зовут `effective` (агент без
   формулы) — слои пака/формулы и варианты в explain не видны. Нужен параметр `--formula` → `Config::for_run(project,
   rig, formula)` в `aae-front/src/api/config.rs` и `aae/src/main.rs` (зона WP-I).
4. Ошибку codex на `gpt-6-1-terra` не получил (см. п. 2 «Сделано»); вопрос владельцу — в журнале.

## Расхождения с DESIGN-agents.md (решения [Д], записаны в журнал)

- harness и provider из разных слоёв, не сочетаются → сильный слой стоит, второй выводится (`derived`); ошибка — только
  если оба в одном слое. Иначе пример раздела 4 (вариант `provider = "openai"` над агентом с `harness = "claude"`)
  был бы ошибкой.
- Ошибки сочетания — при загрузке по каждой prompt-формуле × риг, файл — `aae.toml` проекта, в тексте ключ и слой
  каждого значения (а не файл слоя). Агент, которого не запускает ни одна формула, не проверяется.
- `harness` в `agent.toml` хранится полем `Agent.harness` (не в `Settings`), чтобы не ломать front; в слой агента
  подставляется при сборке. Для вариантов и остальных слоёв — обычный ключ `Settings`.
- Провайдер, которого не знает ни один адаптер, ловится по прогону, не по слою (scripted-харнесс тестов называет свой).
- Умолчание `[harness.<h>] model` берётся у итогового харнесса.
- `aae doctor` и `reload` смотрят агентов как их запускают формулы (плюс агентов отдельно в doctor).

## Правки спеки (для исполнителя документации)

§6.3 — слои 7–9 (пак, формула, вариант) и правило пары harness/provider; §8.1 — `harness` агента необязателен, это
умолчание; новый пункт про варианты (`[agents.<name>]`, `[rigs.<r>] agents`); §7/§2.1 п.6 — адаптер объявляет
провайдеров и сокращения; contracts 7 — ключ модели из конфига до первой попытки.

## Изменённые файлы

`crates/aae-core/src/config/{assemble,check,compose(new),files,items,load,mod,variants(new)}.rs`,
`crates/aae-core/src/driver/adapter.rs`, `crates/aae-core/src/run/{gates,machine,render,reload}.rs`,
`crates/aae-core/src/tests/{config,config_launch(new),mod}.rs`, `crates/aae-claude/src/{adapter,tests}.rs`,
`crates/aae-claude/tests/{claude,contract}.rs`, `crates/aae-codex/src/{adapter,tests}.rs`,
`crates/aae-codex/tests/{codex,common/mod}.rs`, `crates/aae/src/{doctor,harness}.rs`,
`crates/aae-testkit/tests/model_gate.rs` (new), `docs/design/{config,adapters,contracts}.md`, `.loop/JOURNAL.md`.

Проверки: clippy `aae-core aae-claude aae-codex aae aae-testkit` чисто; тесты этих крейтов 392/392; core-boundary OK;
`aae config check --config demos/config.toml` чисто (демо грузятся, `role_prompt.rs` зелёный). Полный `mise run ci` не
гонял.

## Контекст на момент отчёта: ~200k

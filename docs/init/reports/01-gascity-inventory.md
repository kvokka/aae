# 01 — Инвентаризация кода Gas City: что есть, сколько весит, что ядро, а что нет (H7, H11)

Снимок: `/home/ubuntu/proj/work/gascity` @ `f1f75f53c` (main, 2026-11-10). Все цифры — строки Go по `wc -l`
(включая комментарии и пустые строки), если не сказано иное. Скрипты и сырые данные:
`tmp/gc-alt-research/lab/inventory/` (`all_go_files.tsv`, `classify_all.awk`, `classified.tsv`, `classify_cmdgc.awk`).
Классификация `cmd/gc` — эвристика по префиксам имён файлов (первое совпадение regex), погрешность оцениваю в ±5 п.п. на класс **[ВЫВОД]**.

## 1. Выводы за 60 секунд

- **Размер [ПРОВЕРЕНО]:** 931 666 строк не-тестового Go (`internal` 591 828 + `cmd` 339 838) и 1 755 614 строк тестов (1.88:1).
  Из не-тестового 58 652 — один сгенерированный файл `internal/api/genclient/client_gen.go`; ~202 тыс. строк (23%) — комментарии.
- **Доли по классам [ВЫВОД по измеренным строкам]:** ЯДРО 7.4% (69 тыс.), НУЖНО-НО-РАЗДУТО 49.6% (462 тыс.), ПЕРИФЕРИЯ 22.4% (208 тыс.),
  НАСЛЕДИЕ/МИГРАЦИЯ 15.6% (145 тыс.), ТЕСТ-ИНФРАСТРУКТУРА в не-тестовых файлах 5.1% (47 тыс.).
- **Для сценария владельца** (один хост, claude/codex/pi, риги, формулы/ордера, биды) минимум 43% кода (ПЕРИФЕРИЯ + МИГРАЦИЯ + ТЕСТ-ИНФРА ≈ 401 тыс.)
  вообще не участвует; оценка «минимального ядра» с нуля — **70–115 тыс. строк**, т.е. 8–12% нынешнего.
- **Эмпирический якорь [ПРОВЕРЕНО]:** 2026-04-01 весь Gas City (tmux, acp, k8s, API, формулы, dispatch, ордера, mail, convergence) весил **111.5 тыс.** строк.
  За 7 месяцев тот же набор концептов вырос в 7.8 раза.
- **«Наследие старой архитектуры» — неверный диагноз [ПРОВЕРЕНО].** Репозиторию 8.5 месяцев (первый коммит 2026-02-22), это «fresh binary, not a Gas Town fork»
  (`AGENTS.md:167`); упоминаний Gas Town в не-тестовом Go — 155 строк. Код раздут не унаследованным, а **новым**: незавершёнными миграциями, hardening'ом и hosted/enterprise-функциями.
- **Главный структурный источник веса — Dolt/bd [ВЫВОД]:** ~135 тыс. строк (14.5%) существуют, чтобы эксплуатировать Dolt (36 тыс.) и чтобы уносить от него
  инфраструктурные биды в SQLite (99 тыс.: split-store, storebinding, дублирующие бэкенды).
- **Одновременно живут двойные реализации [ПРОВЕРЕНО]:** 2 reconciler'а (legacy tick и keyed v2), formula v1 и v2, ≥5 бэкендов bead store, 8 runtime-провайдеров + 2 роутера,
  `runtime.Provider` и его «seams» (7 файлов `cutover.go`), 7 rollout-гейтов, 2 официальные «active migration» границы в `AGENTS.md`.
- **Город владельца сидит на «старых» путях [ПРОВЕРЕНО]:** `/home/ubuntu/proj/share/gc/city.toml:104` `provider = "tmux"`, `:112` `session_reconciler = "legacy"`.
  17 тыс. строк v2-reconciler'а и ~99 тыс. строк split-store для него сейчас — мёртвый груз.
- **Темп [ПРОВЕРЕНО]:** 7 768 коммитов за 8.5 мес.; октябрь 2026 — 1 075 коммитов, **+223 тыс.** не-тестовых и +550 тыс. тестовых строк нетто за месяц.
  81% последних 300 коммитов имеют `Co-authored-by: Claude Opus`. Половину коммитов за 90 дней сделал один человек.
- **Архитектура «болит» в жизненном цикле сессий [ПРОВЕРЕНО]:** в последних 2 000 коммитах 1 087 `fix` против 305 `feat` (3.6:1); 280 фиксов — session/reconciler/controller,
  147 — storage/dolt, 137 — runtime/tmux.
- **Периферийные провайдеры не заброшены [ПРОВЕРЕНО]:** herdr, k8s, ssh, t3bridge, exec, acp — все тронуты в последние 3 недели. Гипотеза «лежат мёртвым кодом» по git не подтверждается
  (работают ли они — не проверял).
- **Вклад в H11:** по объёму переписать ядро реально; вырезать его из Gas City форком — нет: `cmd/gc` это один `package main` на 338 тыс. строк / 721 файл,
  в нём сконцентрировано 168 тыс. строк прироста за 90 дней. Риск переписывания — не объём, а 1 087 фиксов знаний о гонках и crash-recovery.

## 2. Факты

### 2.1. Общие размеры [ПРОВЕРЕНО]

| Что | Строк | Источник |
|---|---:|---|
| `internal/**` не-тест | 591 828 (128 каталогов верхнего уровня, 202 Go-пакета) | `find internal -name '*.go' ! -name '*_test.go' \| xargs cat \| wc -l`; `go list ./internal/...` |
| `cmd/**` не-тест | 339 838 (из них `cmd/gc` 338 060 в 721 файле) | то же для `cmd` |
| Тесты `internal` / `cmd` | 906 433 / 849 181 | то же с `-name '*_test.go'` |
| Сгенерированный клиент | 58 652 | `wc -l internal/api/genclient/client_gen.go` |
| Комментарии / пустые (без genclient) | 202 502 / 58 579 | `grep -hcE '^\s*//'`, `'^\s*$'` |
| Dashboard (TS/TSX) | 90 793 | `find … -name '*.ts' -o -name '*.tsx' \| xargs wc -l` (`internal/api/dashboardspa`) |
| `openapi.json` | 1.84 МБ, 175 `operationId` | `wc -c`, `grep -o '"operationId"'` |
| Markdown / shell / python | 188 253 / 37 345 / 8 282 | `git ls-files '*.md' …` |
| Бинарник `gc` | 179 МБ | `ls -la …/bin/gc` |
| Файлов >1000 строк / >2000 строк | 176 / 49 | `awk` по `all_go_files.tsv` |

Крупнейшие файлы: `cmd/gc/session_reconciler.go` 8 128, `cmd/gc/build_desired_state.go` 7 911, `internal/runtime/tmux/tmux.go` 6 923,
`cmd/gc/session_lifecycle_parallel.go` 5 828, `cmd/gc/city_runtime.go` 5 580, `internal/config/config.go` 5 366.

Для сравнения **[ПРОВЕРЕНО]** (клоны в `src/`): Gas Town — 248 970 не-тестовых строк Go (@466aa41), bd (Go beads) — 476 809.
Gas City в 3.7 раза больше системы, из которой «извлекал» подсистемы.

### 2.2. Таблица подсистем

Колонки: не-тест / тест, строк. Класс — мой **[ВЫВОД]**; строки — **[ПРОВЕРЕНО]** (`classified.tsv`).

**ЯДРО — 68 626 / 135 659 (7.4%)**

| Подсистема | не-тест | тест | Что делает |
|---|---:|---:|---|
| `internal/dispatch` | 12 293 | 38 852 | control dispatcher формул v2: check/retry/fan-out/tally/drain/finalize |
| `internal/formula` | 10 265 | 18 234 | парсинг и компиляция формул в граф |
| `internal/git, worktree, workdir, rig, citylayout, fsys, clock, shellquote, agent, agentutil` (10 пак.) | 9 412 | 12 285 | worktree-изоляция, provisioning ригов, пути города |
| `internal/events` | 8 710 | 12 362 | event bus поверх `.gc/events.jsonl`, типизированные payload'ы |
| `internal/sling` + `convoy` | 6 305 | 13 106 | маршрутизация работы агенту, группы работ |
| `internal/runtime/acp` | 6 119 | 11 484 | ACP-провайдер (JSON-RPC по stdio) |
| `internal/mail` + `nudgequeue` | 5 145 | 8 880 | почта как биды, отложенные nudge |
| `internal/molecule` | 3 733 | 8 759 | материализация рецепта формулы в биды |
| `internal/orders` | 3 260 | 5 367 | парсинг ордеров, триггеры |
| `internal/hooks` | 1 952 | 3 646 | установка hook-файлов харнесса (Claude, Codex, Gemini, OpenCode…) |
| `internal/runtime/subprocess` | 1 432 | 2 684 | провайдер «дочерний процесс без терминала» |

**НУЖНО-НО-РАЗДУТО — 462 365 / 1 008 818 (49.6%)**

| Подсистема | не-тест | тест | Что делает |
|---|---:|---:|---|
| `cmd/gc`: session reconciler (legacy+общее), desired state, pools/allocator, lifecycle, runtime glue, trace | 86 005 | 232 295 | `session_*`, `build_desired_state*`, `allocator_*`, `pool*`, `runtime_*`, `drain_*`, `cmd_session*` |
| `cmd/gc`: work routing CLI | 67 005 | 165 122 | `cmd_hook_claim.go` 4 634, `order_dispatch.go` 4 695, `cmd_nudge.go` 3 981, `cmd_convoy_dispatch.go` 3 981, `cmd_mail.go` 3 272, `wisp_gc.go` 1 964 |
| `internal/*` мелкая обвязка (49 пак.) | 38 462 | 61 996 | convergence 4 447, materialize 3 374, supervisor 2 443, packman 2 408, readywork 2 105, workladder 1 916, configedit 1 788, beadmeta 1 758 … |
| `cmd/gc`: controller/supervisor/city runtime/CLI-каркас | 35 280 | 80 219 | `city_runtime.go` 5 580, `cmd_supervisor*.go` 7 530, `main.go` 2 182 |
| `cmd/gc`: bd/Dolt lifecycle + passthrough | 29 381 | 73 693 | `dolt_*` (40 файлов), `bd_env.go` 2 690, `beads_provider_lifecycle.go` 4 251 |
| `internal/beads` (Store, BdStore, CachingStore, Mem/File/exec) | 29 236 | 73 432 | `caching_*` 9 710 в 15 файлах, `bdstore*` 5 814 |
| `internal/config` | 27 712 | 57 852 | `city.toml`/`pack.toml`, композиция паков, 746 toml-тегов, `config.Agent` — 71 поле |
| `internal/session` | 24 962 | 54 972 | bead-backed lifecycle сессий, chat, named, submit, wait |
| `cmd/gc`: config/pack/rig/init/import/skills/mcp CLI | 24 775 | 44 554 | `cmd_init.go` 2 436, `rig_import.go` 1 869, `mcp_integration.go` 1 138 |
| `internal/runtime/tmux` | 20 400 | 48 309 | tmux-провайдер, screen-scraping, per-harness interaction |
| `internal/sessionlog` | 17 011 | 17 510 | чтение JSONL-транскриптов Claude/Codex |
| `internal/worker` | 15 188 | 28 869 | «worker boundary», каталог 18 builtin-харнессов (`builtin/profiles.go:247…940`) |
| `internal/runtime` (верх + proctable, registry) | 15 064 | 18 321 | `Provider` (19 методов), 66 интерфейсов, `dialog.go` 2 225, fingerprint 1 077 |
| `internal/doctor` + `cmd/gc` doctor/drift | 24 857 | 39 483 | ~123 типа проверок |
| `internal/beads/contract` + `internal/dolt*` (8 пак.) | 7 027 | 12 191 | резолв Dolt-соединений, doltauth/doltpool/doltorphan/fsyncprobe… |

**ПЕРИФЕРИЯ — 208 370 / 250 372 (22.4%; без сгенерированного клиента 149 691 = 16.1%)**

| Подсистема | не-тест | тест | Что делает |
|---|---:|---:|---|
| `internal/api` (HTTP+SSE, huma) | 60 149 | 114 593 | `huma_*` 16 865 в 54 файлах, `handler_*` 13 618 |
| `internal/api/genclient` | 58 679 | 170 | сгенерированный Go-клиент |
| `internal/productmetrics, telemetry, commandcensus` | 17 167 | 30 273 | продуктовая телеметрия, OTel |
| `internal/runproj, workspacesvc, packregistry, reliability, usage, pricing, buildimage, eventfeed, remotesource` | 12 885 | 12 049 | проекция runs для dashboard, реестры паков, цены, docker-образы |
| `internal/extmsg, webhook*, githubmonitor` | 8 576 | 8 791 | внешние мессенджеры, вебхуки, PR-мониторинг |
| `internal/cliauth, client*, citywriteauth, connect, eventfact, credentialprovider, gitcred, ssrf` | 8 460 | 11 546 | hosted-сервис, удалённые «боксы», учётные данные |
| `cmd/gc`: metrics/login/remote/github/extmsg/dashboard/costs | 7 998 | 13 667 | CLI к перечисленному |
| `cmd/gc`: `api_state*` | 5 094 | 10 610 | состояние контроллера для API |
| `internal/api/dashboardbff, dashboardspa` | 4 980 | 7 504 | BFF и раздача SPA |
| `internal/runtime/t3bridge` | 4 963 | 5 567 | T3 WebSocket |
| `internal/runtime/herdr` | 4 422 | 7 625 | herdr CLI |
| `internal/runtime/exec` | 3 831 | 8 379 | провайдер-скрипт (RPP) |
| `internal/runtime/k8s` | 2 890 | 6 379 | Kubernetes |
| `internal/runtime/programstatus` | 2 806 | 3 767 | OSC 7501 Program Status |
| `internal/runtime/ssh` | 2 133 | 3 386 | SSH |
| `internal/runtime/hybrid, auto` | 1 559 | 3 921 | роутеры local/remote и tmux/ACP |
| `cmd/gen-*`, `openapi-breaking`, `gc-write-mint`, `gc-osc7501` | 1 778 | 2 145 | генераторы и вспомогательные бинарники |

**НАСЛЕДИЕ/МИГРАЦИЯ — 144 964 / 275 831 (15.6%)**

| Подсистема | не-тест | тест | Что делает |
|---|---:|---:|---|
| `cmd/gc`: split-store / infra-class / storage migration / compat | 44 864 | 82 692 | `infra_class_migrate.go` 3 526, `infra_class_recover.go` 2 058, `cmd_bd_by_id.go` 2 304, `storage_*`, `class_*`, `cross_*` (98 файлов) |
| `internal/storebinding, storeref, coordclass` | 30 472 | 39 939 | «work-vs-infrastructure split»: `migration_*` 6 329 в 10 файлах, `sqlite/legacy_combined.go` 923 |
| `internal/beads`: native/proxied/doltlite/sqlite + proxyendpoint + ledgerfeed | 23 779 | 46 255 | альтернативные бэкенды рядом с BdStore |
| `cmd/gc`: `reconcile_*` | 17 297 | 48 590 | keyed reconciler v2 (69 файлов), живёт рядом с legacy |
| `internal/opjournal` (+`legacyop` 1 531) | 13 400 | 22 016 | conversation operation journal, shadow-режим |
| `cmd/gc`: conversation lanes + opjournal glue | 10 301 | 26 597 | `conversation_lane_*`, `op_record.go` |
| `internal/bdflags, rollout, migrate, bootstrap, featureflags` | 4 851 | 9 742 | флаги bd по версиям, rollout-гейты, миграция PackV1 |

Допущение: из двух reconciler'ов «миграционным» засчитан v2 (17 тыс.), потому что legacy-only строки внутри `session_reconciler.go` отделить без трассировки нельзя **[НЕ ПРОВЕРЕНО]**;
по `ROADMAP.md:32` удалят как раз legacy, так что реальное дублирование не меньше меньшей из двух частей.

**ТЕСТ-ИНФРАСТРУКТУРА в не-тестовых файлах — 47 341 (5.1%)** плюс 52 249 строк тест-онли харнессов в `cmd/gc`

| Подсистема | не-тест | Что |
|---|---:|---|
| `internal/testpolicy, testutil, testenv, testfixtures, bazeltest, chartest, formulatest` | 13 487 | бюджеты шардов, resource census, timing |
| `internal/beads/beadstest, splittest` | 13 103 | conformance-наборы для store |
| `internal/runtime`: runtimetest, runtimecontract, runtimecapability, rppcheck, `fake*.go`, `fakeacp` | 11 845 | conformance провайдеров, фейки |
| `internal/storebinding/storebindingtest` | 4 109 | |
| `internal/worker/workertest, fake, fakecmd` | 3 623 | |
| `internal/events/eventstest`, `session/sessiontest` | 1 174 | |

Не вошли: `tools/` 5 875 (nogo-анализаторы, 13 штук), `test/` 11 607 не-тест + 73 078 тест, `scripts/` 1 819 Go + 37 тыс. shell.

### 2.3. Признаки миграций и двойных путей в цифрах [ПРОВЕРЕНО]

| Признак | Число | Пример |
|---|---|---|
| Runtime-провайдеры | 8 + 2 роутера, 47 749 строк; tmux — 43% | `engdocs/architecture/nine-concepts.md:74`; `internal/runtime/{tmux,acp,subprocess,exec,k8s,ssh,herdr,t3bridge,auto,hybrid}` |
| Бэкенды bead store | BdStore, native, proxied, sqlite, doltlite, Caching, Mem, File, exec | `internal/beads/` префиксы файлов; `nine-concepts.md:95` |
| Reconciler'ы | 2 | `cmd/gc/reconcile_mode.go:22-23`; `internal/config/config.go:3266-3267`; 19 ветвлений `runsV2()` в 5 файлах |
| Formula-контракты | 2 (v1 «legacy global-setter path», v2) | `internal/rollout/registry.go:66`; `internal/config/config.go:2877-2878` (`GraphWorkflows` — deprecated-предшественник) |
| Rollout-гейты | 7 (сроки смерти 2026-12-31 … 2027-06-30) | `internal/rollout/registry.go:22,42,58,71,85,101,117` |
| «Active migration» границы в CI | 2: worker boundary, typed store boundary | `AGENTS.md:144,147` |
| Долг typed-store границы | 1 910 записей, 2 252 нарушения в shrink-only baseline; в `cmd/gc` одних `raw-param` 1 092 | `tools/nogo/analyzers/storeclass/baseline.txt`, `ceiling.txt` |
| Разделение `runtime.Provider` на seams | 7 файлов `cutover.go` | `internal/runtime/acp/cutover.go:10` («serves the legacy runtime.Provider through the de-conflated seams») |
| Shadow-режимы | program status, ops journal, nudge debts | `internal/runtime/program_status.go:38`; `internal/rollout/registry.go:94`; `cmd/gc/nudge_shadow_debts.go` |
| Совместимость с gc-enterprise | deprecated-алиасы `off/auto/require` | `internal/config/config.go:3000,3294` |
| PackV1 → V2 imports | `legacy_*` в config, 429 строк | `internal/config/legacy_v1_surfaces.go:13-22` |
| Не-тестовые файлы с `legacy/compat/migrat/cutover/shadow/relic/backfill` в имени | 60 файлов, 22 056 строк | `find … \| grep -iE … \| xargs wc -l` |

Grep по не-тестовому Go (`grep -rIic`, строк / файлов из 2 406): `legacy` 4 832 / 626; `migrat` 2 785 / 345; `fallback` 2 463 / 481; `shadow` 1 456 / 182;
`compat` 508 / 191; `deprecated` 227 / 66; `cutover` 134 / 48. Слово `legacy` встречается в каждом четвёртом файле.

Совместимость с Gas Town/bd: Gas Town — 155 строк, в основном имена builtin-паков (`internal/builtinpacks/registry.go:69`); bd — отдельный пакет `internal/bdflags` (1 985)
и 67 упоминаний минимальных версий. То есть слой совместимости с Gas Town мал; слой сопряжения с bd/Dolt — велик.

### 2.4. Динамика [ПРОВЕРЕНО]

Срезы (`git grep -c '' <ref>`), не-тест без genclient:

| Дата | не-тест | пакетов `internal/*` | тест |
|---|---:|---:|---:|
| 2026-03-01 | 33 391 | 17 | 43 062 |
| 2026-04-01 | 111 503 | 32 | 126 900 |
| 2026-06-01 | 287 687 | 70 | 488 174 |
| 2026-08-01 | 458 814 | 108 | 793 899 |
| 2026-10-01 | 608 665 | 117 | 1 116 894 |
| 2026-11-01 | 827 648 | 126 | 1 630 575 |

- Коммитов по месяцам (`git log --date=format:%Y-%m`): фев 326, мар 1 077, апр 1 397, май 966, июн 848, июл 738, авг 500, сен 544, **окт 1 075**, ноя (10 дней) 297. За последние 30 дней — 927 (≈31/день).
- Нетто не-тестовых строк в месяц (`git log --numstat`): 60–113 тыс. в мае–сентябре, **+223 тыс. в октябре**, +52 тыс. за 10 дней ноября.
- Hot spots за 90 дней (нетто не-тест): `cmd/gc` +168 281, `internal/runtime` +47 081, `internal/beads` +40 930, `internal/api` +15 520, `internal/opjournal` +13 400, `internal/session` +12 318.
  Половина нынешнего `cmd/gc` написана за последние 3 месяца.
- Новых каталогов `internal/*` за 90 дней: 18 (opjournal, connect, eventfact, workladder, readywork, worktree …); `storebinding` появился 2026-08-06.
- Авторы: 265 за всё время, 84 за 90 дней; из 2 239 коммитов за 90 дней топ-1 (Julian Knutsen) — 49%, топ-2 — 64%, три бота — 15%. Bus factor ≈ 1–2 **[ВЫВОД]**.
  В последних 300 коммитах 244 с `Co-authored-by: Claude Opus 5.5`.
- Провайдеры, последний коммит / коммитов за 90 дней: tmux 11-08 / 133; acp 10-31 / 40; herdr 11-09 / 35; auto 11-02 / 35; exec 11-10 / 29; hybrid 10-22 / 28;
  t3bridge 11-04 / 23; subprocess 11-10 / 20; ssh 11-01 / 18; k8s 11-08 / 14. tmux получает в 3.3 раза больше внимания, чем acp.
- Последние 300 коммитов по типам: fix 176, feat 52, test 31, ci 15, docs 7. Последние 2 000 (с 2026-08-27): fix 1 087, feat 305, test 201, ci 100, perf 52.
- Фиксы последних 2 000 по областям (группировка scope'ов — моя): session/reconciler/controller lifecycle 280; storage/beads/dolt/split-store 147; runtime/tmux/nudge 137;
  dispatch/orders/formula/sling/hook 105; ci/test/build 105; config/pack/cli/doctor 84; api/dashboard/events 49; прочее/без scope 180.

### 2.5. Почему кода так много — структурные причины

1. **Dolt как хранилище и побег от него.** 36 тыс. строк только на управление сервером (`cmd/gc/dolt_*` — 40 файлов: порты, блокировки data-dir, orphan-процессы, disk preflight)
   и ~99 тыс. на перенос инфраструктурных бидов в SQLite (`ROADMAP.md:48`). Сверху — `CachingStore` (9 710 строк), потому что читать Dolt на каждом тике дорого.
2. **Ничего не удаляется до конца миграции.** Два reconciler'а, два контракта формул, провайдер и его seams, `ops`-журнал в shadow рядом с «legacy delivery».
   Каждая миграция приносит ещё и doctor-проверки, preflight'ы и rollout-гейт.
3. **Проекция API + OpenAPI + dashboard.** 60 тыс. рукописного сервера, 58.7 тыс. сгенерированного клиента, 5 тыс. BFF, 90.8 тыс. TS, 175 операций. Одна Go-часть (124 тыс.) больше, чем весь Gas City в апреле.
4. **Screen-scraping TUI.** `internal/runtime/dialog.go` (2 225) распознаёт девять классов стартовых диалогов (`dialog.go:47`), включая rate-limit (`dialog.go:1568`);
   `tmux/interaction*.go` 2 123, отдельные `claude_background_stop.go` 364 и `codex_background_stop.go` 275, плюс OSC 7501 (2 806) как попытка уйти от скрейпинга.
5. **Множественность всего.** 8 провайдеров × conformance-наборы (11.8 тыс. не-тестового кода только на их проверку), 18 builtin-харнессов, 9 реализаций Store, 66 интерфейсов в `internal/runtime` и 60 в `internal/beads`.
6. **Hardening конкурентности.** CAS-фенсы, leases, incarnation, drain-ack, circuit breaker, trace reconciler'а (3 980 строк). Соотношение fix:feat 3.6:1 показывает, что это реакция на реальные отказы, а не украшательство.
7. **Doctor и самодиагностика.** 24.9 тыс. строк, ~123 проверки, заметная часть — про состояние миграций (`doctor_v2_*`, `doctor_cross_store`, `doctor_rollout_gates`).
8. **Hosted/enterprise и телеметрия в OSS-бинарнике.** productmetrics 14.6 тыс., connect/eventfact/cliauth/citywriteauth, webhooks, extmsg.
9. **Монолитный `package main`.** 338 тыс. строк в одном пакете без внутренних границ: доменная логика (reconciler, allocator, order dispatch) лежит в CLI-слое, а не в `internal/`.
10. **Стиль генерации.** 23% строк — комментарии со ссылками на дизайн-документы и id бидов; 63 top-level команды и 312 литералов `cobra.Command`; тестов вдвое больше кода.

### 2.6. Внешние зависимости [ПРОВЕРЕНО] (`go.mod`: 41 прямая, 205 indirect)

| Зависимость | Где используется | Зачем |
|---|---|---|
| `github.com/steveyegge/beads` v1.3.4 | `internal/beads` (20 файлов), `eventfact`, `ledgerfeed` | in-process «native» store; тянет indirect весь Dolt (`dolthub/dolt/go`, `go-mysql-server`, `vitess`), AWS/Azure/GCP SDK, Arrow |
| `go-sql-driver/mysql` | `cmd/gc`, `doltpool`, `proxyendpoint`, `api` | SQL-доступ к Dolt-серверу |
| `modernc.org/sqlite` | `internal/beads`, `storebinding/sqlite` | SQLite для инфра-классов |
| `k8s.io/{api,apimachinery,client-go}` | только `internal/runtime/k8s` (3 файла) | k8s-провайдер |
| `go.opentelemetry.io/otel/*` (8 модулей) | только `internal/telemetry` (4 файла) | метрики и логи OTLP |
| `danielgtaylor/huma/v2`, `pb33f/libopenapi*`, `oapi-codegen/runtime`, `invopop/jsonschema` | `internal/api` (31 файл), genclient, docgen | типизированный API и спецификация |
| `gorilla/websocket` | только `internal/runtime/t3bridge` | T3 |
| `go-jose`, `golang-jwt` | только `internal/webhookverify` | подписи вебхуков |
| `shirou/gopsutil` | только `internal/api/dashboardbff` | метрики хоста |
| `gastownhall/gascity-packs` | `builtinpacks`, `config`, `cmd/gc` | встроенные паки |
| `cobra`, `BurntSushi/toml`, `fsnotify`, `x/sys`, `uuid`, `semver`, `backoff` | повсюду | базовое |

Для целевого сценария из 41 прямой зависимости нужны ~8–10 **[ВЫВОД]**. 179 МБ бинарника — в основном следствие линковки Dolt через `steveyegge/beads` **[ВЫВОД, размеры по секциям не мерил]**.

## 3. Вердикты

### H7 — «кода много больше, чем используется; большая часть — наследие» — ПОДТВЕРЖДЕНА ЧАСТИЧНО

- **Первая половина подтверждена.** Для сценария владельца минимум 43% не-тестового кода не участвует (ПЕРИФЕРИЯ 22.4% + МИГРАЦИЯ 15.6% + ТЕСТ-ИНФРА 5.1%).
  Внутри оставшихся 57% значительная часть обслуживает tmux и Dolt, от которых владелец хочет уйти.
  Его город при этом использует `tmux` + `legacy` reconciler + один Dolt-store, т.е. даже «новые» 17 тыс. строк v2 и ~99 тыс. строк split-store не исполняются (`city.toml:104,112`).
- **Вторая половина опровергнута по формулировке.** Это не наследие Gas Town и не «старая архитектура»: 87% нынешнего объёма появилось после 2026-04-01, проект начат с нуля.
  Точнее так: **Gas City застрял в середине нескольких собственных миграций одновременно** и несёт обе стороны каждой. Плюс слой hosted-продукта, который одиночному хосту не нужен.
- Оговорка: класс «НУЖНО-НО-РАЗДУТО» (49.6%) не равен «лишнему». Заметная его часть — оплаченные инцидентами знания о гонках и восстановлении (1 087 фиксов за 2.5 месяца).

### Оценка «минимального ядра» [ВЫВОД]

| Функция | Сейчас (internal + `cmd/gc`) | На 2026-04-01 | Оценка с нуля |
|---|---:|---:|---:|
| config + композиция паков | 27.7k + 24.8k | 6.9k | 8–12k |
| адаптер bead store (один бэкенд + лёгкий кэш) | 71.3k + 29.4k | 4.0k | 4–8k |
| модель сессии + reconciler + пулы | 25.0k + 103k | 2.3k + часть `cmd/gc` | 12–20k |
| runtime: ACP + subprocess + интерфейс | 6.1k + 1.4k + 15.1k | 1.4k + 0.6k + 1.3k | 6–10k |
| formula + dispatch (+molecule) | 26.3k | 10.4k | 12–20k |
| orders | 3.3k + ~6k | 0.5k | 2–4k |
| events | 8.7k | 1.7k | 2–4k |
| mail + nudge | 5.1k + ~11k | 1.9k | 3–5k |
| sling / hook / claim / convoy | 6.3k + ~20k | — | 5–8k |
| rig / git / worktree | 9.4k | 0.6k | 3–5k |
| controller + supervisor + CLI-каркас | 35.3k | — | 6–10k |
| doctor | 24.9k | 1.9k | 2–3k |
| harness hooks + чтение транскриптов | 19.0k | 2.4k | 3–5k |
| **Итого** | | 111.5k (весь репо, с API 13.6k, extmsg 3.8k, k8s 1.7k) | **~70–115k** |

Обоснование: нижняя граница — апрельский Gas City без периферии (~85 тыс.) с поправкой на то, что ACP убирает screen-scraping, а br убирает управление сервером БД;
верхняя — запас ×1.3–1.5 на hardening, который апрельская версия ещё не прошла. Тестов при нынешнем соотношении понадобится ещё 130–220 тыс.

### H11 — вклад

- **За переписывание.** Ядро мало (8–12% нынешнего объёма). Форк с вырезанием дорог: домен лежит в `cmd/gc` (`package main`, 721 файл) без пакетных границ;
  typed-store долг — 1 910 записей; 69 файлов `cmd/gc` импортируют `internal/api`. Upstream прирастает на 60–220 тыс. строк в месяц, форк не удержать в синхронизации.
- **Против / риски.** 280 фиксов session/reconciler lifecycle за 2.5 месяца — в основном транспортно-независимые задачи (кто владеет сессией, двойной claim, orphan'ы, drain).
  Переписывание встретит их заново. Bus factor upstream ≈ 1–2, но у переписанной системы он будет ровно 1.
- **Нейтрально.** Периферийные провайдеры активно коммитятся — тезис «только на бумаге» по git не подтверждается и не опровергается.

## 4. Следствия для дизайна и варианты

1. **Вариант A — форк + вырезание.** Убрать ПЕРИФЕРИЮ можно сравнительно дёшево там, где она в отдельных пакетах (`runtime/{k8s,ssh,herdr,t3bridge}`, productmetrics, extmsg — ~60 тыс.).
   МИГРАЦИЮ вырезать нельзя без завершения самих миграций: она вплетена в `cmd/gc`. Итог — ~500–600 тыс. строк чужого быстро меняющегося кода. Не рекомендую.
2. **Вариант B — с нуля, бюджет 70–115 тыс. строк.** Один store, один транспорт, один reconciler, один контракт формул. Правило из наблюдений: **не начинать вторую реализацию, пока не удалена первая** —
   именно его нарушение дало 15.6% веса.
3. **Вариант C — с нуля, но совместимо по форматам.** Сохранить `pack.toml`/`city.toml`/формулы/ордера как входной формат (терминология та же), чтобы паки владельца переносились.
   Цена — придётся воспроизвести часть `internal/config` (27.7 тыс. сейчас, 6.9 тыс. в апреле).
4. **Апрельский срез как скелет.** Теги `v0.13.x`–`v0.15.1` (март–апрель) — компактная (≈110 тыс.) версия той же архитектуры; читать её как референс дешевле, чем HEAD.
5. **Тесты как спецификация отказов.** 1.76 млн строк тестов и 1 087 fix-коммитов — готовый каталог сценариев гонок и восстановления. Стоит выписать из них список инвариантов до начала дизайна (отдельная задача).
6. **Держать домен вне CLI-пакета** и не строить типизированную API-проекцию, пока нет второго клиента: API+клиент+dashboard сегодня — 215 тыс. строк.
7. **Что точно не брать:** управление сервером БД, split-store, shadow-журналы, rollout-гейты, conformance-наборы для сторонних провайдеров, продуктовую телеметрию, OSC 7501.

Замечание для смежных гипотез (не моя тема, но попалось): в Gas City **есть** установка harness-hooks (`internal/hooks`, 1 952 строки, форматы под Claude/Codex/Gemini/OpenCode/Copilot)
и MCP-интеграция (`cmd/gc/mcp_integration.go` 1 138, `internal/session/mcp*.go` 333). H9 в части «хуков нет» стоит перепроверить по этим файлам.

## 5. Открытые вопросы и непроверенное

- **Покрытие исполнением не мерил.** «Не участвует в сценарии» выведено из назначения пакетов и конфига города, а не из coverage-профиля живого `gc`. Доли — оценка сверху по структуре, не по трассе.
- **Legacy-only строки reconciler'а** не отделены от общих; дублирование оценено снизу (17 тыс.).
- **Классификация `cmd/gc`** — по префиксам имён; файлы со смешанным назначением (например, `build_desired_state.go`) целиком отнесены к одному классу.
- **Работоспособность периферийных провайдеров** (herdr, k8s, ssh, t3bridge) не проверялась — только активность в git.
- **Насколько CLI зависит от API-сервера** в одиночном хосте: 69 файлов `cmd/gc` импортируют `internal/api`, но какие команды реально ходят через HTTP — не трассировал.
- **Оценка 70–115 тыс.** — экспертная; она не учитывает объём, который добавят новые функции владельца (общий реестр MCP, control pane через ACP).
- **Вклад Dolt в размер бинарника** (179 МБ) — предположение, `go tool nm`/`bloaty` не запускал.
- CONTEXT называет 129 пакетов в `internal/`; `ls -d internal/*/` даёт 128 каталогов (202 Go-пакета с подпакетами).

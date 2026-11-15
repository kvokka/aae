# WP-69 · Паки из git: sha, тег, последняя ревизия — план для кодера

Спека: §8.2, §11.6, §12, §13; решение 85; критерий 26; допущение §18 п.16. Код продукта, комментарии, тексты ошибок,
доки — только по-английски. Номера строк ниже не даю (в `docs/` идут параллельные коммиты) — ищи по заголовкам и grep.

## 1. Что есть сейчас и что уходит

| Что | Где | Судьба |
|---|---|---|
| Команда `aae fetch [--update]` | `crates/aae/src/fetch.rs` (145 строк), `Command::Fetch` в `crates/aae-front/src/cli/mod.rs`, ветка в `crates/aae/src/main.rs` | уходит целиком; `clone()` и `git()` из `fetch.rs` переезжают в новый модуль почти как есть |
| `aae.lock` (`Lock`, `Pinned`, `LOCK_FILE`), `checkout(dir, commit)` → `<каталог файла>/.aae/git/<commit>/`, сверка `.git/HEAD` | `crates/aae-core/src/config/sources.rs` | уходят |
| `GitSource { git, rev: String }`, `rev` = ветка, тег или коммит | там же; `Source::Git` и `PluginDecl { path, git, rev }` в `config/files.rs` | `rev` становится `Option<String>`: полный sha или тег; ветка больше не принимается |
| `git_sources(file) -> Result<Vec<(PathBuf, BTreeSet<GitSource>)>, _>` (по каталогам, ради lock-файлов) | `sources.rs` | остаётся, но плоским и без ошибок: `-> BTreeSet<GitSource>` |
| `Loader::root()` читает lock и каталог checkout; загрузка git не запускает; «не скачано» — ошибка «run `aae fetch`» | `sources.rs` | берёт каталог из готовой таблицы, которую передал вызывающий |
| Все загрузки конфига идут через одну функцию `harness::load(paths)` | `crates/aae/src/harness.rs`; зовут её старт демона и замыкание `load` (reload, `POST /v1/config/check`) в `daemon.rs`, `doctor.rs`, `config check\|explain` в `main.rs` | сюда и встаёт скачивание — одно место, вне ядра |
| Каталога кеша нет | `crates/aae-front/src/dirs.rs` знает только state и socket | добавляется `cache()` |
| Тесты | `crates/aae/tests/fetch.rs`, `crates/aae-core/src/tests/config_sources.rs` | переписываются |
| Тексты | `docs/design/config.md`, `docs/guide/plugin.md`, `docs/api/frontends.md`, `README.md` | см. раздел 5 |

Размеры (wc -l, сверь перед работой): `config/files.rs` — 343 (не 400; расти ему и не нужно — меняется один комментарий),
тесные — `config/load.rs` 392 и `config/mod.rs` 386 (план добавляет +2 и +3), `cli/mod.rs` 363 (+2),
`aae-acceptance/src/checks.rs` 381 (+0), `scenarios/mod.rs` 341 (+7). Ядро: 10 211 из 18 000, после пакета станет меньше.

## 2. Целевое поведение

Синтаксис (ключи не меняются, `rev` становится необязательным):

```toml
packs = [
  { git = "https://github.com/acme/kit", rev = "v1.2.0" },   # tag
  { git = "https://github.com/acme/kit2", rev = "<40 or 64 hex>" }, # full commit id
  { git = "https://github.com/acme/tools" },                  # no pin: the latest revision
]
[[plugin]]
git = "https://github.com/acme/aae-plugin-x"
rev = "v2"            # обязателен: без него — ошибка конфига (§11.6)
```

- `rev` — полный commit id в нижнем регистре (`commit_id()`: 40 или 64 hex) → закрепление по sha; любой другой текст →
  имя тега, ищется только как `refs/tags/<rev>`; имя ветки или короткий sha — ошибка «нет такого тега».
- `rev` нет → последняя ревизия = `HEAD` удалённого репозитория (вершина ветки по умолчанию), выясняется при каждой
  загрузке конфига.
- Кеш: `$XDG_CACHE_HOME/aae/git` (иначе `~/.cache/aae/git`), один на пользователя. Раскладка:
  - `<cache>/<key(url)>/<commit>/` — checkout (detached), после переименования на место не меняется;
  - `<cache>/<key(url)>/tag-<key(tag)>` — текстовый файл: коммит, на который тег указал при скачивании;
  - `<cache>/<key(url)>/head` — текстовый файл: коммит последней виденной вершины.
  `key(text)`: байты `[A-Za-z0-9_-]` и `.` (кроме первой позиции) как есть, остальные — `%XX`. Так ни `..`, ни `/` в имя
  каталога не попадают.
- Сеть:
  - sha: каталог `<commit>/` есть → git не запускается вообще;
  - тег: файл `tag-…` называет коммит и каталог есть → git не запускается (теги неизменяемы: передвинутый тег не замечается
    до очистки кеша);
  - без закрепления: `git ls-remote -- <url> HEAD` при каждой загрузке; коммит есть в кеше → обновить `head`, клона нет;
    нет → клон; `ls-remote` не удался и `head` называет существующий каталог → работаем на нём с предупреждением в stderr
    процесса, который грузил конфиг; `head` нет → ошибка конфига;
  - всё, чего в кеше нет, скачивается: `git clone --quiet --no-checkout -- <url> <temp>`, разрешение ревизии, `checkout
    --detach`, переименование `<temp>` → `<commit>/`.
- Скачивает бинарный крейт `aae` до вызова ядра; ядро получает таблицу «источник → каталог либо причина» и git не
  запускает.
- `aae cache clear` — без демона, удаляет весь `<cache>`; следующая загрузка конфига скачивает заново.
- Плагин из git без `rev` — ошибка конфига в ядре (с именем файла, как прочие).

## 3. Изменения по файлам

Порядок: 3.1 → 3.2 → 3.3 одним коммитом (иначе не соберётся), затем тесты 4.1–4.2 в тот же или следующий коммит, строка
приёмки — отдельным, доки — отдельным, READINESS/PLAN/журнал — последним.

### 3.1 Ядро `crates/aae-core/src/config/`

`sources.rs` (170 → ~80 строк):
- Удалить: `LOCK_FILE`, `Pinned`, `Lock` с `impl`, `checkout()`, импорты `Serialize`, `fs`, `io`.
- `GitSource`: `pub rev: Option<String>`; rustdoc: «pinned by `rev` to a tag or a full commit id; without it, the latest
  revision».
- Новое: `pub type Checkouts = BTreeMap<GitSource, Result<PathBuf, String>>;` — «where each git source was downloaded
  to, or why it was not».
- `commit_id()` остаётся `pub` (им пользуется бинарник).
- `git_sources(orchestrator_file: &Path) -> BTreeSet<GitSource>`: тот же обход (файл оркестратора, затем `aae.toml`
  проектов), результат плоский, `loader.errors` отбрасываются (их целиком сообщит настоящая загрузка; если бы функция
  возвращала ошибку, загрузка добавила бы к ней ложное «не скачано»).
- `Loader::plugin_source`: `(None, Some(git), Some(rev))` → `Source::Git`; `(None, Some(_), None)` → ошибка
  ``a plugin from git needs `rev`, a tag or a full commit id: only a pack may follow the latest revision``; прочее —
  прежнее ``a plugin has either `path`, or `git` with `rev` ``.
- `Loader::root`: `Source::Path` как было; `Source::Git(git)` → `self.checkouts.get(git)`:
  `Some(Ok(dir))` → `Some(dir.clone())`; `Some(Err(why))` → ошибка `` `<url>`: <why> ``; `None` → `` `<url>` is not
  downloaded `` (достижимо только когда загрузке не дали таблицу — тесты ядра). Параметр `base` для git больше не нужен.
- Шапка модуля: ядро git не запускает; скачивает тот, кто грузит конфиг.

`load.rs` (+2): поле `pub(super) checkouts: super::Checkouts` в `Loader` (Default — пустая таблица).

`mod.rs` (+3): `pub use sources::{Checkouts, GitSource, commit_id, git_sources};`; `load_for(orchestrator_file, adapters,
checkouts: Checkouts)` кладёт таблицу в `Loader`; `load()` передаёт `Checkouts::new()`.

`files.rs` (±0): только комментарий над `Source` («a directory, or a git repository at a tag, a commit or its latest
revision»). `PluginDecl` и `expecting = "a pack is a path or `{ git, rev }`"` не трогать.

Вызовы `load_for` с двумя аргументами (добавить третий `Default::default()`): `crates/aae-core/src/tests/config.rs`
(`load_for_adapters`), `crates/aae/tests/role_prompt.rs`.

### 3.2 Бинарник `crates/aae/`

`git mv crates/aae/src/fetch.rs crates/aae/src/git_cache.rs` и переписать (~130 строк, `mod git_cache;` в `main.rs`):
- `pub(crate) fn checkouts(cache: &Path, config: &Path) -> Checkouts` — для каждого из `git_sources(config)` зовёт
  `checkout`. Весь проход под `static LOCK: Mutex<()>`: в демоне reload, `config/check` и doctor могут грузить конфиг
  одновременно, а временный каталог клона назван по pid.
- `fn checkout(cache: &Path, source: &GitSource) -> Result<PathBuf, String>` — логика раздела 2. Три случая дают
  «что искать в клоне» (`<sha>`, `refs/tags/<tag>`, `HEAD`) и «какой указатель записать» (нет, `tag-…`, `head`); общий
  хвост: `create_dir_all(repo)`, клон во `<repo>/.clone-<pid>`, `rename` в `<repo>/<commit>` если его нет, убрать temp,
  записать указатель. Если `rename` не удался, а каталог назначения уже есть (другой процесс успел) — это успех.
- `fn clone(temp, url, want: &str) -> Result<String, String>` — нынешний `clone()`: `rev-parse --verify --quiet
  --end-of-options <want>^{commit}`, проверка `commit_id`, `checkout --quiet --detach`, сверка `rev-parse HEAD` — сохранить.
  Ошибки: тег — ``no tag `<rev>` in the repository; `rev` is a tag or a full commit id``, sha — ``no commit `<sha>` in the
  repository``.
- `fn git(args)` — как есть, плюс `.env("GIT_TERMINAL_PROMPT", "0")` (у демона нет терминала: запрос пароля иначе
  повиснет). Комментарий: `// shortcut: no timeout of its own, a remote that never answers holds the load until git
  gives up; add one when a reload must not wait.`
- `fn key(text: &str) -> String` — экранирование из раздела 2.
- Предупреждение: `eprintln!("aae: warning: `{url}`: the latest revision is not known ({e}); using `{commit}` from the
  cache")`.
- `pub(crate) fn clear(cache: &Path, json: bool) -> ExitCode` — `fs::remove_dir_all`; `NotFound` — тоже успех; печать
  `cleared <dir>` либо `{"cleared":"<dir>"}` при `--json`; иная ошибка — `cli::fail(json, "io_error", …)`.

`harness.rs` (+2): в `load(paths)` перед `config::load_for`: `let checkouts = crate::git_cache::checkouts(&paths.cache,
file);` и передать третьим аргументом.

`daemon.rs` (+3): поле `pub(crate) cache: PathBuf` в `Paths`, в `resolve` — `cache: dirs::cache()?`; в замыкании `load`
обернуть вызов: `tokio::task::block_in_place(|| load(&paths))` — git может идти секунды, а обработчики reload и check
зовут замыкание прямо из async (doctor уже так делает в `api/daemon.rs`; рантайм демона многопоточный).
`daemon_tests.rs` (+1): поле `cache` в литерале `Paths`.

`main.rs` (±0): ветку `Command::Fetch` заменить на
`Command::Cache(CacheCommand::Clear) => return match aae_front::dirs::cache() { Ok(dir) => git_cache::clear(&dir,
cli.json), Err(e) => cli::fail(cli.json, "config_error", &format!("cache clear: {e}")) }` — `Paths::resolve` тут не
нужен (он требует существующий `--config`).

### 3.3 Фронт `crates/aae-front/`

`dirs.rs` (+5): `pub fn cache() -> Result<PathBuf, String>` = `xdg("XDG_CACHE_HOME", ".cache")?.join("aae/git")`,
rustdoc «The packs and plugins downloaded from git».

`cli/mod.rs` (+2 нетто):
- удалить вариант `Fetch { config, update }`;
- добавить `/// The cache of packs and plugins downloaded from git.` `#[command(subcommand)] Cache(CacheCommand)` и
  `#[derive(Subcommand)] pub enum CacheCommand { /// Remove every downloaded pack and plugin; the next load of the config
  downloads them again. Clear }`;
- шапка модуля и ветка `execute` (список команд без демона и текст ошибки): `fetch` → `cache clear`.

## 4. Тесты

Везде, где тест запускает `git` или `aae`: `HOME` и `XDG_CACHE_HOME` — во временном дереве, `GIT_CONFIG_GLOBAL=/dev/null`,
`GIT_CONFIG_NOSYSTEM=1`; коммиты — `git -c user.name=t -c user.email=t@t`. «Сеть» — локальный путь к репозиторию;
«сети нет» — репозиторий переименован.

Изоляция кеша (обязательно, иначе при выставленном у хоста `XDG_CACHE_HOME` тест сотрёт настоящий кеш):
`.env("XDG_CACHE_HOME", dir.join("cache"))` в `command()` файла `crates/aae-acceptance/src/daemon.rs` и в `command()`
файла `crates/aae/tests/common/mod.rs`.

### 4.1 Ядро: `crates/aae-core/src/tests/config_sources.rs` (переписать, ~100 строк)

Свой помощник: пишет файлы `BASE` + изменения во временный каталог и зовёт `crate::config::load_for(&file, &[],
checkouts)` (таблице нужны абсолютные пути этого каталога, поэтому `load_with` не подходит).
1. `a_git_source_loads_from_the_directory_it_was_downloaded_to` — пак и плагин объявлены из git, их файлы лежат в
   произвольных каталогах (`dl/kit`, `dl/mail`), таблица называет их: `plugins[0].root` и `packs[1].root` равны этим
   каталогам, формула пака видна. Пустая таблица — две ошибки `is not downloaded`. `Err("boom")` в таблице — ошибка с
   url и `boom`.
2. `a_plugin_from_git_must_be_pinned` — `[[plugin]] git = "x"` без `rev` → ошибка «needs `rev`»; пак `{ git = "x" }` без
   `rev` той же ошибки не даёт.
3. `git_sources_names_every_declared_source` — `git_sources()` возвращает плагин оркестратора (с `rev`) и пак проекта
   (`rev: None`); файл проекта с ошибкой разбора не мешает вернуть источники оркестратора.
4. Оставить `a_plugin_has_a_path_or_a_git_source` (убрать случай `git` без `rev` — он теперь в п.2) и
   `a_pack_with_an_unknown_key_says_what_a_pack_is`.
Удалить тесты про lock и `.git/HEAD`.

### 4.2 Бинарник: `git mv crates/aae/tests/fetch.rs crates/aae/tests/git_cache.rs` (переписать, ~130 строк)

Без демона: `aae config check --json` и `aae cache clear`; помощники `isolated`, `git`, `repo`, `commit` оставить.
1. `a_rev_that_is_no_tag_and_no_commit_is_a_config_error` — `rev = "main"` (ветка), `rev` = 40 hex, которых нет в
   репозитории, и несуществующий url: `config check` падает, ошибка называет файл `aae.toml` и url; в кеше не осталось
   каталогов `.clone-*`.
2. `a_tag_in_the_cache_is_not_asked_again` — тег `v1`, загрузка; тег передвинут (`git tag -f v1` на новый коммит),
   репозиторий затем переименован; загрузка проходит, в `<cache>/<key>/` ровно один каталог-коммит — старый.
3. `without_a_pin_the_cache_serves_when_the_remote_is_away` — пак без `rev`: загрузка, новый коммит, загрузка — в кеше оба
   коммита; репозиторий переименован — `config check` успешен, в stderr `warning` с url; после `aae cache clear` тот же
   `config check` падает.
4. `cache_clear_needs_no_daemon_and_no_cache` — `aae cache clear` дважды подряд успешен (сокета нет), каталог кеша исчез.

### 4.3 Строка §15: `crates/aae-acceptance/src/scenarios/c26_git.rs` (~120 строк), `Launch::Ci`

Регистрация: `mod c26_git;` и `Scenario { criterion: "26", title: "a pack from git by tag, by sha and unpinned: aae
downloads it; a cached tag or sha needs no network; `cache clear` makes it download again", launch: Launch::Ci, run:
|env| Box::pin(c26_git::run(env)) }` в `scenarios/mod.rs`; `"26"` в `ROWS` в `checks.rs` (в ту же строку массива).

Сценарий (образец — `c25_return.rs`; дерево и демон — `Daemon::start`, пути паков — `GET /v1/packs`, поле `root`):
1. В `before`: три репозитория в `<root>/remote/{tagged,pinned,latest}`, в каждом `pack.toml` со своим именем и
   `formulas/one.toml` (exec `true`). `tagged`: коммит A, тег `v1`, затем коммит B. `pinned`: коммит A (sha запомнить),
   затем B. `latest`: коммит A. `shop/aae.toml`: `packs = [{git, rev="v1"}, {git, rev="<sha A>"}, {git}]` плюс прежний
   `[rigs.web]`.
2. Демон поднялся — тест ничего не скачивал сам. `root` паков `tagged` и `pinned` кончаются на их коммит A (не B),
   `latest` — на его вершину; все три — каталоги под `<tree>/cache/aae/git`.
3. Новый коммит C в `latest`, `POST /v1/config/reload` → `root` пака `latest` кончается на C.
4. `remote` переименован в `remote.off`. Reload успешен, `root` те же. В `daemon.log` есть предупреждение с путём
   `remote/latest` и нет строк с `remote/tagged` и `remote/pinned` — к тегу и sha не обращались.
5. `d.cli(&["cache", "clear"])` успешен, каталога кеша нет. Reload теперь — ошибка (`d.post(..).is_err()`): взять неоткуда.
6. `remote.off` → `remote`. Reload успешен, все три `root` снова существуют — скачаны заново.

Запуск одной строки: `cargo build -q -p aae && cargo run -q -p aae-acceptance -- 26`.

### 4.4 Условие готовности

`grep -rn 'aae\.lock\|aae fetch' crates docs` пуст (сейчас совпадения только в файлах из раздела 1 и в трёх доках); заодно
`grep -n 'aae fetch\|aae\.lock' README.md` пуст. Затем `scripts/loop/checks 80`.

## 5. Доки

1. `docs/design/config.md`:
   - «Loading», первый пункт: перед чтением `aae` скачивает git-источники в кеш; так же поступают `aae config
     check|explain`, `aae doctor`, `POST /v1/config/check`; последняя ревизия незакреплённого пака выясняется при каждой
     загрузке.
   - пример «Project: `<project>/aae.toml`»: комментарий у git-пака — «pinned to a tag or a full commit id»; добавить
     строку `{ git = "…" }` — «no pin: the latest revision of the default branch».
   - раздел переименовать ровно в `### Packs and plugins from git` (так его называет спека §8.2) и переписать: синтаксис;
     что такое `rev`; кто и когда скачивает; каталог и раскладка кеша; три правила сети; предупреждение и где оно видно;
     теги неизменяемы; `aae cache clear` (при работающем демоне после неё — `aae config reload`); плагин обязан быть
     закреплён; lock-файла и команды закрепления нет; правки в кеше не замечаются; нужен `git` в `PATH`, доступ — средствами
     самого git, без запросов пароля; у git-операций нет своего таймаута.
   - абзац о доверии после раздела: «…a plugin from git runs only at the tag or commit its declaration pins».
2. `docs/guide/plugin.md`, абзац «A plugin in a git repository of its own…»: `git` + `rev` (тег или полный commit id,
   обязателен), скачивает `aae` при загрузке конфига, ссылка на раздел config.md.
3. `docs/api/frontends.md`, раздел «CLI»: в списке команд без демона `aae fetch` → `aae cache clear`; одна фраза, что
   команда делает.
4. `docs/guide/install.md`, таблица «Directories»: строка `packs and plugins from git | $XDG_CACHE_HOME/aae/git
   (~/.cache/aae/git); emptied by aae cache clear`.
5. `README.md`: пункт про `aae fetch` заменить пунктом про git-паки и `aae cache clear`.
6. `docs/READINESS.md`: строка «Config assembly» и абзац оценки (у критерия 26 теперь есть строка), маркер `MEASURE` — на
   коммит не раньше последней правки `crates/` (как сделано для критерия 25, коммит `f78b26c`).
7. `.loop/PLAN.md`: статус WP-69; `.loop/JOURNAL.md`: запись РЕШЕНИЕ с допущениями раздела 6.
Спеку и `docs/api/openapi.json` не трогать: API не меняется.

## 6. Допущения [Д] и риски

Допущения, принятые там, где спека молчит (перенести в журнал одной записью):
1. [Д] Ключ остаётся `rev`: полный commit id — sha, иначе тег; ветка и короткий sha не принимаются. Отдельные ключи `tag` /
   `sha` отвергнуты: больше кода и правок при том же смысле.
2. [Д] Раскладка кеша и файлы-указатели `tag-…`, `head` (раздел 2). Это служебное состояние кеша, не lock-файл: в проекте
   ничего не появляется, закрепление — только в конфиге.
3. [Д] Команда — `aae cache clear`, без аргументов, чистит всё (в кеше лежат и плагины, поэтому не `aae pack …`).
   При работающем демоне его текущий конфиг указывает на удалённые каталоги до `aae config reload` — сказано в доке.
4. [Д] Предупреждение о работе на ревизии из кеша — строка в stderr процесса, грузившего конфиг (журнал демона, stderr
   `config check`); в ответы API не попадает.
5. [Д] Любой отказ `git ls-remote` считается «сети нет». Последняя ревизия выясняется при любой загрузке конфига, включая
   `config check` и doctor.
6. [Д] Полный клон на каждую новую ревизию, `.git` остаётся в checkout; экономия места и общий объектный склад — не сейчас.
7. [Д] Своего таймаута у git-операций нет, только `GIT_TERMINAL_PROMPT=0`.

Риски:
- **Главный: загрузка конфига теперь может ждать сеть.** Reload держит мьютекс `applied`, старт демона идёт до первого
  ответа git. Смягчение — `block_in_place` и отсутствие запросов пароля; настоящего таймаута нет (допущение 7). Если
  ревьюер сочтёт это блокером — опрос `try_wait` с дедлайном в `git()`, около 10 строк.
- Чистка кеша под работающим демоном ломает хуки плагинов из git и чтение файлов пака до reload.
- `XDG_CACHE_HOME` в тестах: без строки из раздела 4 тест может удалить настоящий кеш пользователя.
- Длинный url: `key()` утраивает спецсимволы, имя каталога ограничено 255 байтами — выйдет ошибка конфига от файловой
  системы. Принято.
- Отвергнутый вариант: ядро получает замыкание-загрузчик вместо таблицы. Короче на ~20 строк (нет второго разбора файлов в
  `git_sources`), но ядро само инициировало бы скачивание — против буквы §13 «ядро читает только готовые каталоги».
- Проверено опытом на git 2.55 (локальный репозиторий): `ls-remote -- <путь> HEAD` даёт `<sha>\tHEAD`; в клоне
  `--no-checkout` разрешаются `refs/tags/v1^{commit}` (аннотированный тег), `HEAD^{commit}`, `<sha>^{commit}`; ветка как
  `refs/tags/<имя>` отвергается; после переименования репозитория `ls-remote` выходит с 128; у пустого репозитория вывод
  пуст при коде 0 — разбор обязан требовать `commit_id`.

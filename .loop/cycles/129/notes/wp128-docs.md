# WP-128, docs: оставшиеся вхождения `plugin` (все — нативный смысл или неизменяемое)

- docs/agents/decisions.md:141 — решение 125: плагин нативной системы плагинов харнесса (Claude Code, Codex).
- docs/agents/decisions.md:145 — решение 141: «importing the native plugins of Claude Code or Codex».
- docs/agents/decisions.md:146 — решение 142: «The plugin of the operator's harness», путь обновлён на `plugin/`.
- docs/agents/decisions.md:149 — решение 145 целиком, не тронуто.
- docs/api/frontends.md:118 — навыковый плагин оператора для Claude Code/Codex (`plugin/`).
- docs/architecture.md:62-65 — `plugin/` как плагин харнесса оператора, `.claude-plugin/`, `plugin.json`, `.agents/plugins/marketplace.json`.
- docs/design/config.md:951 — каталог плагина Claude Code (`--plugin-dir`); дальше в строке 952 — addon.
- docs/design/contracts.md:485 — флаг claude `--plugin-dir`.
- docs/design/contracts.md:488 — «the skills plugin» процесса claude (нативный плагин сессии).
- docs/design/contracts.md:566 — app-server codex синхронизирует свои плагины (репозиторий плагинов вендора).
- docs/design/experiments.md:306,307,309,348 — `--plugin-dir`, `.claude-plugin/plugin.json`, навыки и хуки плагина claude.
- docs/design/experiments.md:310 — «4 plugins stay»: встроенные плагины claude.
- docs/design/experiments.md:543 — `src=.loop/research/2026-11-18-plugins` в маркере MEASURE: имя существующего каталога исследования, не переименовывается.
- docs/design/experiments.md:548 — навыки `--plugin-dir`, префикс плагина, `agents/` плагина claude.
- docs/design/experiments.md:602 — хуки и плагины opencode.
- docs/guide/addon-development.md:105 — «a Claude Code plugin directory (`--plugin-dir`)».
- docs/guide/recovery.md:10,12,15,16 — плагин aae для Claude Code/Codex, его marketplace, `claude plugin install`, `codex plugin add`.
- demos/4-context-budget/RESULT-example.md:59 — проза «The compactions the plugin counted» оставлена по поручению (менялись только ключи, слой `addon context-budget` и путь); run.sh теперь печатает «the addon counted».

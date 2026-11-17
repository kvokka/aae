# Plugin formats: Claude Code 2.1.326 and Codex CLI 0.168.0 (verified in a temp dir, temp CLAUDE_CONFIG_DIR/HOME/CODEX_HOME)

Docs: https://code.claude.com/docs/en/plugins-reference.md , /plugin-marketplaces.md ;
https://developers.openai.com/codex/plugins/build.md (describes newer root `plugin.json`; legacy `.codex-plugin/plugin.json` and Claude-compatible manifests also accepted).

## Result: ONE layout works for both, no codex-specific files needed
repo/
  .claude-plugin/plugin.json        {"name":"demo-plugin","version":"0.1.0","description":"Demo plugin with one skill"}
  .claude-plugin/marketplace.json   {"name":"demo-mkt","owner":{"name":"Me"},"plugins":[{"name":"demo-plugin","source":"./","description":"Demo"}]}
  skills/demo/SKILL.md              (frontmatter: name, description; body below)
Codex installed it from only `.claude-plugin/*` (tested with `.codex-plugin/` absent; with it present
containing {"name","version","description","skills":"./skills/"} it is redundant).
Codex docs also list repo marketplace `.agents/plugins/marketplace.json` (entry: {"name","source":{"source":"local","path":"./x"},"policy":{"installation":"AVAILABLE","authentication":"ON_INSTALL"},"category":"..."}) and say `.claude-plugin/marketplace.json` is read as legacy-compatible -- confirmed by `codex plugin list`.

## Claude
- plugin.json optional; only `name` required (kebab-case). Optional: displayName, version, description, author{name,email,url}, homepage (must be URL), repository, license, keywords, skills (paths, adds to default `skills/`), commands, agents, hooks, ...
- Skills: `skills/<name>/SKILL.md` default location. Frontmatter used: name, description.
- Marketplace `.claude-plugin/marketplace.json`: requires name, owner{name}, plugins[]; entry needs name + source (relative "./..." path, no ".."; or {"source":"github","repo":..}, git-subdir, url...). Entry name must equal plugin.json name. source "./" (marketplace root = plugin) validated and installed fine.
- Commands (all passed, offline):
  claude plugin validate <dir>            -> "Validation passed with warnings" (only ICON_MISSING directory warning), rc 0
  claude plugin validate <dir> --strict   -> fails only because of that icon warning
  claude plugin validate <dir> --profile directory -> fails (submission checks); irrelevant for local install
  claude plugin marketplace add <local path | owner/repo | git url> [--scope user|project|local]
  claude plugin install demo-plugin@demo-mkt [-s scope]   (or: install demo-plugin --marketplace <source>)
  claude plugin list ; claude plugin marketplace list/update/remove
  A marketplace is needed for install. (`--plugin-dir <dir>` at session start loads without one; not tested.)

## Codex
- Commands (passed, offline, no model/login):
  codex plugin marketplace add <local path | owner/repo[@ref] | https/ssh git URL> [--ref R] [--sparse P]   (file:// is rejected)
  codex plugin list ; codex plugin add demo-plugin@demo-mkt (or `demo-plugin --marketplace demo-mkt`) ; codex plugin remove ; codex plugin marketplace list/upgrade/remove
  No `validate` subcommand.
- Writes $CODEX_HOME/config.toml ([marketplaces.demo-mkt] source_type="local"; [plugins."demo-plugin@demo-mkt"] enabled=true) and copies the repo to $CODEX_HOME/plugins/cache/<mkt>/<plugin>/<version>/ (incl. .git).
- Skills discovered from root `skills/` (no `skills` field needed). Frontmatter: name, description.
- Harmless warning with CODEX_HOME under /tmp: "could not create PATH aliases".
- Git-URL marketplace add not tested (no remote); same path as `owner/repo`.

## Pitfalls
- `mise exec` hangs when HOME is overridden: use absolute binaries
  /home/ubuntu/.local/share/mise/installs/claude/2.1/claude and .../codex/0/bin/codex.
- Plugin is cached per version; bump `version` for updates.

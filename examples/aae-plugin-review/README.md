# aae-plugin-review

An example external plugin for aae, used by acceptance scenario 15. See `plugin.toml` for how to declare and
attach it, and `docs/design/config.md` (Plugin) for the format.

- `skills/review-checklist`: a skill for the agents that attach the plugin.
- `hooks/remind.sh`: a harness hook (`UserPromptSubmit`) for those agents.
- `prompt.md`: a fragment appended to those agents' system prompt.
- `bin/require-word`: an orchestrator hook on `run.finishing` that vetoes a pass whose summary lacks the word of
  the `require` parameter. It reads the event JSON with `sed` and `grep` to stay dependency-free: a summary
  or parameter with a quote in it breaks that parsing, so a real hook should use a JSON parser such as `jq`. As
  everything in `bin/`, it is also on the `PATH` of the attached agents' sessions.

To attach it from git, give it a repository of its own (aae reads `plugin.toml` at the root of a checkout) and
declare it with a revision instead of a path; `aae fetch` pins that revision to a commit in `aae.lock` next to the
declaring file and checks it out, and the pin moves only with `aae fetch --update`:

```toml
[[plugin]]
git = "https://github.com/acme/aae-plugin-review"
rev = "v0.1.0"
config = { require = "tested" }
```

# aae-plugin-review

An example external plugin for aae, used by acceptance scenario 15. See `plugin.toml` for how to declare and
attach it, and `docs/design/config.md` (Plugin) for the format.

- `skills/review-checklist`: a skill for the agents that attach the plugin.
- `hooks/remind.sh`: a harness hook (`UserPromptSubmit`) for those agents.
- `bin/require-word`: an orchestrator hook on `run.finishing` that vetoes a pass whose summary lacks the word of
  the `require` parameter.

# agent-mail changelog

## Unreleased

- Coordinate claude and codex runs of one repository through one Agent Mail server the daemon runs: a mail name per
  run and agent, a shared thread, path reservations, a refused commit of a path another run holds, and the release of
  a run's reservations when it ends. Needs `mcp-agent-mail` 0.3.40 or later, `jq`, `curl` and Git with configured
  hooks.

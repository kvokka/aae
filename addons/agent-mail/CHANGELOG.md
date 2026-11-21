# agent-mail changelog

## 0.2.0

- Set the `bind` attachment parameter to publish a stable MCP port. Requires a loop-manager build with `[[service]].bind`.
- An explicit `HTTP_ALLOW_LOCALHOST_UNAUTHENTICATED=true` now allows anonymous loopback calls. Empty and false
  require the bearer supplied by the daemon. Remote MCP clients still use the bearer.

- Coordinate claude and codex runs of one repository through one Agent Mail server the daemon runs: a mail name per
  run and agent, a shared thread, path reservations, a refused commit of a path another run holds, and the release of
  a run's reservations when it ends. Needs `mcp-agent-mail` 0.3.40, `jq`, `curl` and Git with configured
  hooks.

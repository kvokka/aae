# conventional-commits

Attach this addon to a coding agent working in a Git repository. The prompt gives one example.
Git checks the subject before creating a commit. It accepts a lowercase type, optional scope and
breaking-change marker, and a description. With a bead the subject must end in its literal ID in
square brackets, e.g. `feat(api)!: change the response format [dev-42]`. Bodies and trailers are optional.

The addon needs jq and Git with configured hook support (`git hook list`). It registers a named
`commit-msg` hook through the session's Git configuration, alongside repository hooks.
The check applies only to the run's Git repository, so test fixtures can create their own commits.

## Install

In your project's `loop-manager.toml`, declare the Git source and attach the addon to the project's agents.
If you already have a `[settings]` table, append the attachment to its `addons` array.

```toml
[[addon]]
git = "https://github.com/kvokka/loop-manager.git"
rev = "2278ccdcccba31f65560c8ed5b039036e0e65b01"  # conventional-commits 0.1.0
dir = "addons/conventional-commits"

[settings]
addons = [{ name = "conventional-commits" }]
```

Run `loop-manager config check`, then `loop-manager config reload` to apply the change on the project's daemon.
To enable the addon for a single agent, put the attachment in that agent's `agent.toml` and keep the Git
declaration in the project file.

See the [changelog](CHANGELOG.md) for version history.

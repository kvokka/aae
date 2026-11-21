---
name: addons
description: Discover available loop-manager addons and their versions, install or update a pinned addon, roll it back, or create a custom addon in the user’s own repository.
---

# Work with loop-manager addons

## Discover addons and versions

For “what addons are available?”, inspect the current default branch of the
[catalog repository](https://github.com/kvokka/loop-manager/tree/HEAD/addons).
Enumerate `addons/*/addon.toml` and read each manifest and README for its name, purpose, harnesses and host
requirements. Use the repository API or a fresh Git checkout. For a user-supplied repository, inspect that
source and locate the manifest at its root or in the requested subdirectory. Local `loop-manager addon list`
shows what the user's configuration declares and what is bundled. Answer discovery requests without editing config.

For catalog releases, list real tag refs for the selected name:

```sh
git ls-remote --refs --tags "$url" "refs/tags/addons/$name/*"
```

Write the published tag in `rev` so the selected version is readable. Honor an explicit commit pin. For an unspecified version choose the highest stable published
SemVer for that addon (exclude prereleases unless requested). Read the manifest, README and changelog at
the selected revision and verify the manifest name and version. External publishers may use their own tags.
If a requested version has no matching tag, report the available choices. When only untagged content exists,
identify it as such and offer its verified full commit SHA as a development snapshot.

## Apply the user's selection

1. Locate the target project and its orchestrator config. Check existing declarations and attachments.
   For an external install, add or update one `[[addon]]` declaration with `git`, exact `rev` and
   repository-relative `dir` (omit `dir` for a root manifest). For development inside the same repository,
   use a `path` relative to the declaring config. Names must be unique across orchestrator and project.
2. Attach the manifest name in the requested settings layer. Use project `[settings]` for a project-wide
   install or the agent's settings for that agent alone. Merge into the existing `addons` array and
   preserve order, params and unrelated entries. For an update or rollback, change the existing pin,
   check the changelog for parameter migrations and account for every attachment using that declaration.
3. Validate the edited files with `loop-manager config check --config <orchestrator-config>`. For an agent,
   use `loop-manager config explain --config <orchestrator-config> --project <project> --agent <agent>`
   to verify its effective settings. To activate the install on its running daemon, run
   `loop-manager config reload` against that daemon. Report the pin, scope and whether the change is active.

See the [configuration examples and attachment scopes](https://github.com/kvokka/loop-manager/blob/HEAD/docs/guide/addons.md).

## Create a custom addon

Use the user's repository. Put `addon.toml` at its root for a dedicated addon repository, or in a
self-contained subdirectory for a collection. Choose a name unique in the target project and start the
manifest with `name` and a SemVer `version`. For example, an addon that appends project instructions
needs only this manifest and a sibling `prompt.md` containing the instructions:

```toml
name = "review-notes"
version = "0.1.0"
prompt = "prompt.md"
```

For other behavior, select the relevant extension point from the
[authoring guide](https://github.com/kvokka/loop-manager/blob/HEAD/docs/guide/addon-development.md).
Use `[agent.hooks.<name>]` for session events or `[[hook]]` for orchestrator events. Package referenced
files with the addon, preserve executable bits on hook programs and document required host tools.

Attach the addon in a test project using a relative `path` and run the config check and explain commands
above. Verify its contribution for the target agent and exercise the behavior it adds. Write a README
with installation and usage, and a per-addon `CHANGELOG.md`. When the user wants to distribute it,
follow the [SemVer release procedure](https://github.com/kvokka/loop-manager/blob/HEAD/docs/guide/addon-development.md#versioning-and-releasing-an-addon).
Share the repository URL, the exact tag and the subdirectory if applicable. Users install directly from Git.

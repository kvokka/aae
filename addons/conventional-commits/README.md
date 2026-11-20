# conventional-commits

Attach this addon to a coding agent working in a Git repository. The prompt gives one example.
Git checks the subject before creating a commit. It accepts a lowercase type, optional scope and
breaking-change marker, and a description. With a bead the subject must end in its literal ID in
square brackets, e.g. `feat(api)!: change the response format [dev-42]`. Bodies and trailers are optional.

The addon needs jq and Git with configured hook support (`git hook list`). It registers a named
`commit-msg` hook through the session's Git configuration, alongside repository hooks.
The check applies only to the run's Git repository, so test fixtures can create their own commits.

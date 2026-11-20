# For agents working on this repository

The rules of work on loop-manager itself. The product is described in the rest of [`docs/`](../README.md); this
directory is the kitchen. Read a file when its "when" comes, not all of them at the start.

| File | What is in it | Read it |
|---|---|---|
| [rules-code.md](rules-code.md) | line budgets, no stubs, file size, measurements, dependencies, git | before writing or reviewing code |
| [rules-tests.md](rules-tests.md) | what to run and when, live models, the demos, how tests are written | before running or writing tests, before touching `demos/` |
| [decisions.md](decisions.md) | what the owner decided: not revisited | before changing a behaviour, a config key or an interface |
| [assumptions.md](assumptions.md) | choices that are not the owner's, open questions, what is not verified | the same |
| [loop.md](loop.md) | the development loop, its workflow and agent roles | when you work as a role of the loop |

## What is true

- The product is what the code does and what `docs/` says. There is no spec above them. A document that differs from
  the code is a defect: where the document states a [decision](decisions.md) the code is wrong, otherwise the
  document is, and it is fixed in the same change.
- `docs/` says how loop-manager works, in the present tense. It holds no history: no "was", no cycle or work-package
  numbers, no commit of a change. History is in git.
- New work comes from the owner: an epic, its comments or a direct request. What the owner did not ask for is not done.
- A comment that says "decision 49" means [decisions.md](decisions.md); "criterion 8" means
  [acceptance.md](../acceptance.md).

## Language

The product is English only: code, identifiers, comments, rustdoc, `docs/` (this directory too), `README`, CLI and
error texts, OpenAPI, CI, commit messages. The coder and reviewer write their comments in Russian for the owner, with the headings specified by their roles.

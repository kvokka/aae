---
name: recap
description: Summarize outcomes, setbacks and open decisions across loop-manager runs or Beads issues.
---

# Recap completed work

When a run you are following finishes, give the user a short recap. For a run waiting on a decision,
use [awaiting](../awaiting/SKILL.md) to surface the question while the answer can still advance the run.

## Gather the evidence

For run IDs, use `loop-manager run show <run>` on the user's daemon. Read the project, inputs, outcome,
step results and artifact references. If the run has a bead, locate its project directory with
`loop-manager project list --project <project>`. For bead IDs in a known tracker, start there directly.
For runs without a bead, use the step outcomes and referenced artifacts.

For a br tracker, group the requested beads by project. Read each description, status and full thread once:

```sh
BEADS_DIR='<project dir>/.beads' br show <bead> <another-bead> --format toon
```

Where delegated model selection is available, give the read-only extraction to an inexpensive model.
Pass the tracker, IDs and user's focus. Have it read the source directly and return compact TOON findings
with source IDs. Batch related beads that fit its context. Otherwise do the extraction in the current session.
For long threads, select the needed comments and return TOON:

```sh
BEADS_DIR='<project dir>/.beads' br comments "$bead" --json |
  jq --argjson ids "$comment_ids" 'map(select(.id as $id | $ids | index($id) != null))' |
  loop-manager format
```

Here `comment_ids` is a JSON array of numeric comment IDs.

## Brief the user

Compare the requested outcome with the recorded work and verification. Read comments using the project's own
conventions. On a reused bead, attribute work to the requested run and check later answers, fixes and corrections.
Distinguish a decision the worker already made from a question that still needs the user. Include addon findings
or lessons when they explain a result or a recurring difficulty, whatever heading the addon uses.

Default to a few sentences in the user's language. Lead with anything the user needs to decide, then give the
outcome, the most important difficulty and whether it was resolved. For several beads, identify each outcome
and report shared patterns once. On request, expand with the reasoning, evidence, remaining work and comment IDs.

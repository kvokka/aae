---
name: recap
description: Give the user a brief or detailed account of work recorded on one or more Beads issues. Use at the end of a loop-manager run you are following and when asked what was delivered, what hurt, what needs attention, or what several runs learned.
---

# Recap work from Beads

When a run you are following finishes, include the recap in your reply without waiting for a separate request.
Follow the user's focus and language. If a run is waiting for an owner decision, surface the question now using
the [awaiting procedure](../awaiting/SKILL.md).

## Read the evidence once

For run IDs, get the bead and project from `loop-manager run show <run>` on the selected daemon.
Find the project directory with `loop-manager project list --project <project>`. In this repository
use `devloop/lm` for those commands. For bead IDs and a known tracker, go straight to br.

Group requested IDs by tracker. Read each distinct bead once, including its description, status and full comments.
This command accepts one or several IDs and returns TOON:

```sh
BEADS_DIR='<project dir>/.beads' br show <bead> <another-bead> --format toon
```

Where the host supports delegated model selection, give this read-only extraction to an inexpensive available
model. Pass the tracker, IDs, requested focus and depth. Have it read br directly and return compact TOON
findings with bead and comment IDs so the caller need not ingest the same thread again. Batch beads that fit
in context together. Otherwise apply the same procedure in the current session. For a long thread, select
comments in smaller groups and convert any JSON pipeline back to TOON before returning it to the agent.
See the [devloop comment queries](../awaiting/references/devloop.md#context-and-outstanding-concerns).

## Interpret the thread

- Compare the requested outcome with what the coder delivered and the reviewer verified. Trace significant
  setbacks through later fixes or corrections. For a specific run on a reused bead, attribute work using
  the run's comment authors and consult the rest of the thread for owner answers and later developments.
- In devloop, match each `QUESTION` or `DECISION` with later entries of the same ID across the whole bead.
  Show what still needs an answer and the concrete choice. Describe a discretionary decision as a choice
  already made. A closed bead alone says nothing about owner acceptance.
- Read addon comments alongside worker and owner comments. A `LESSONS: <run>` entry or another addon's
  finding belongs in the answer when it explains the outcome, a material difficulty, or a pattern the user
  asked about. New addon headings require no change to the query. Check later corrections before repeating a claim.

## Reply to the user

Default to a few sentences or short bullets. Lead with any decision the user needs to make now, then say what
changed and the most important difficulty or remaining limitation. Say whether a significant problem was fixed.
If the user asked what needs their attention and the record shows nothing outstanding, say so directly.

For several beads, identify each outcome by bead ID and combine repeated difficulties or lessons once, naming
the affected beads. On request, expand with the relevant sequence of events, choices and reasons, review or CI
evidence, unresolved work and comment IDs. If evidence is missing, identify the gap. Deliver the recap in chat.

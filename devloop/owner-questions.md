# Questions in the devloop pack

Read this for the repository's `devloop` project and `epic` formula. Other packs may use other formats: inspect their
instructions and step outputs instead of imposing these names on them. All commands below run on the machine that
holds the project and its checkout; do not create a new tracker if the recorded project directory is unavailable.

## Context and outstanding concerns

The epic's description is the task. Its comments are the working memory: `CODER` summaries, `REVIEW` reports,
questions, decisions and answers. The worker's `finish` data names a blocking concern with `needs_owner: true` and
`question_id`; it does not contain the whole question. Read the description and comments from the br tracker at
`<project dir>/.beads`, using the directory from `project list`, not an unrelated `.beads` under the rig.

Each concern is a separate comment with an exact first line and a self-contained explanation below it:

- `QUESTION: <id>`: only the owner can decide; work waits after the model session ends.
- `DECISION: <id>`: the worker chose an authorized option and continued; the owner should see the disputed choice.
- `ANSWER: <id>`: the owner's answer to that concern. A later question with the same id can reopen it.

`needs-owner` is a discovery label, not an await or an answer. Neither removing it nor completing an epic accepts a
decision. Only a later matching answer resolves the concern. Read recent coder and review reports along with the
question; check referenced code or artifacts when its wording alone is insufficient.

Use the bead from `devloop/lm run show <run>` for completed runs as well. From the checkout root,
set `BEADS_DIR` to the selected project's tracker. Read the epic in TOON. For a narrower selection,
keep JSON inside the pipeline and return TOON to the caller.

```sh
export BEADS_DIR='<project dir>/.beads'
br show "$bead" --format toon

# One comment by numeric id
br comments "$bead" --json | jq --argjson id "$comment_id" 'map(select(.id == $id))' | devloop/lm format

# Latest reviewer comment from this run. For the coder, use startswith("CODER:").
br comments "$bead" --json |
  jq --arg actor "loop-manager:$run:" 'map(select(
    (.author | startswith($actor)) and (.text | startswith("REVIEW\n"))
  )) | .[-1:]' | devloop/lm format

# Questions, decisions and answers across the whole epic, in order
br comments "$bead" --json |
  jq 'map(select(.text | test("^(QUESTION|DECISION|ANSWER): ")))' | devloop/lm format
```

For each concern id, read the latest matching entry. A final `QUESTION` or `DECISION` still needs attention.
Include closed epics when looking for discretionary decisions with `br list --all -l needs-owner --format toon`.
CI step outcomes are available through `devloop/lm run show <run>`. Full logs and notes are local to the run machine.

## Answer an active wait

Check the current run state immediately before submitting. The wait is `code_owner` for the coder or `review_owner`
for the reviewer. Take its question id from the corresponding `code` or `review` step's data and verify that the
user's answer addresses that concern.

```sh
loop-manager run resolve <run> code_owner pass --summary '<the owner answer>'
```

Use `review_owner` instead for the reviewer's wait. Here `pass` accepts the answer for processing, not the epic's
completion. The following `code_answer` or `review_answer` script writes `ANSWER: <id>` and returns to a fresh coder
round with that answer in its remarks and comments. An opposing choice from the owner still goes through `pass`:
for example, "Use the other format" is an answer. `fail` on these await steps ends the run, rather than sending the
answer to the coder. Cancel only when the user wants to abandon the run.

A tracker comment by itself does not resolve an active wait, and opening an issue gate does not resolve one either.
For a discretionary decision with no active wait, record the owner's response as a comment instead: prepare a file
whose first line is `ANSWER: <id>` and whose following lines contain their answer, then use
`PATH="$HOME/.local/opt/loop-manager-dev/bin:$PATH" \
  BEADS_DIR='<project dir>/.beads' br comments add <bead> -f <answer file>`. Remove `needs-owner` only if all concerns
have answers. An answer on an already completed epic does not start new implementation work automatically.

## After an owner wait times out

The await timeout is seven days in this pack. On expiry the run fails with `reason: await_timeout`, releases the
epic to `open`, and closes its issue gate because `bead_failed_runs = 1`. The question remains unanswered. Do not
describe this as a failed review, and do not attempt to resolve the terminal run.

First reconstruct the question and obtain the owner's answer. Record `ANSWER: <question_id>` on the epic with the
file procedure above, before opening the gate: an automatic run may already be queued. With the user's choice to
continue, inspect the queued runs, open `bead:devloop/<bead>`, and use that existing run if it is already queued.
Only when no replacement run exists, start a new one:

```sh
loop-manager run start devloop repo epic --input epic=<bead>
```

The new coder reads the answer from the epic's comments. It does not attach to the old worker's session. Confirm
which run now owns the task and report that run to the user.

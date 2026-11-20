Review the work on the epic `{{ bead.id }}`: {{ bead.title }}

<epic>
{{ bead.description }}
</epic>

The last comments on the epic, oldest first; `br comments {{ bead.id }}` gives all of them:

<comments>
{{ bead.comments }}
</comments>

The full test run that followed the coder (the step `ci_report`: `mise run ci` on the tree the coder left):

<tests>
{{ steps.ci_report.data.stdout }}
</tests>

The run is `{{ run.id }}`; you work in `{{ rig.path }}`. Review as your role says, leave the review as a comment on
the epic, then finish:
`loop-manager finish pass --summary "SCORE <n>/10, VERDICT <verdict>" --data '{"score": <n>, "verdict": "<verdict>", "needs_owner": false}'`.
If no authorized choice unblocks the work, leave a separate `QUESTION: <id>` comment and add `needs-owner`, then
finish with `verdict: "continue"`, `needs_owner: true` and `question_id: "<id>"` in the data. The formula waits at
`review_owner`; the owner's answer goes to a new coder round.

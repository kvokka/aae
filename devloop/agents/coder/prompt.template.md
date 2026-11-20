Your task is the epic `{{ bead.id }}`: {{ bead.title }}

<epic>
{{ bead.description }}
</epic>

The last comments on the epic, oldest first; `br comments {{ bead.id }}` gives all of them:

<comments>
{{ bead.comments }}
</comments>

Why the workflow returned to you, including any failed CI checks and log path (empty in the first round):

<remarks>
{{ remarks }}
</remarks>

The run is `{{ run.id }}`; you work in `{{ rig.path }}`. Work as your role says: take stock of what is there, then the
review's mandatory items, then the rest of the epic. Before you finish, leave your summary as a comment on the epic.
Then `loop-manager finish pass --summary "<done or partial: what was done>" --data '{"needs_owner": false}'`.
When only an owner's decision can unblock the work, first leave `QUESTION: <id>` and add `needs-owner`, then finish
`pass --summary "blocked: <what must be decided>" --data '{"needs_owner": true, "question_id": "<id>"}'`.
The formula waits for the owner after your session ends; the answer reaches the next round in the comments and remarks.

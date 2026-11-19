Your task is the epic `{{ bead.id }}`: {{ bead.title }}

<epic>
{{ bead.description }}
</epic>

The last comments on the epic, oldest first; `br comments {{ bead.id }}` gives all of them:

<comments>
{{ bead.comments }}
</comments>

What the gate said when it sent the run back to you (empty in the first round):

<remarks>
{{ remarks }}
</remarks>

The run is `{{ run.id }}`; you work in `{{ rig.path }}`. Work as your role says: take stock of what is there, then the
review's mandatory items, then the rest of the epic. Before you finish, leave your summary as a comment on the epic.
Then `loop-manager finish pass --summary "<done or partial: what was done>"`, or
`loop-manager finish fail --summary "blocked: <why>"`.

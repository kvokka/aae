Your task is the bead `{{ bead.id }}`: {{ bead.title }}

<task>
{{ bead.description }}
</task>

The run is `{{ run.id }}`. You work in `{{ rig.path }}`; the files of the run are in `.loop/runs/{{ run.id }}/`
there: your report `report.md`, the review `review.md` once a round was reviewed, the notes of subagents in `notes/`.

The remarks of the checks that sent the run back to you, or of the script that checked your report (none in the
first round):

<remarks>
{{ remarks }}
</remarks>

Do the task as your role says: the report first, then the remarks, then the work, committed. Then finish:
`loop-manager finish pass --summary "<STATUS and what was done>"`, or
`loop-manager finish fail --summary "blocked: <why>"`.

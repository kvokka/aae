Review the work on the bead `{{ bead.id }}`: {{ bead.title }}

<task>
{{ bead.description }}
</task>

The run is `{{ run.id }}`. You work in `{{ rig.path }}`; the coder's report is
`.loop/runs/{{ run.id }}/report.md`, and your review goes to `.loop/runs/{{ run.id }}/review.md`, written anew for
this round. The step `ci` passed on the current HEAD: {{ steps.ci.summary }}.

Review as your role says, then finish: `loop-manager finish pass --summary "SCORE <n>/10, VERDICT <verdict>"`.

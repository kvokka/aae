Your task is the bead `{{ bead.id }}`, in `{{ rig.path }}`:

<task>
{{ bead.description }}
</task>

Work through these steps.

1. Read every Markdown file under `{{ rig.path }}/docs` to its end, one file after another: do not skim, search or
   sample them, the task needs all of their text.

2. Write `{{ rig.path }}/GLOSSARY.md` as the task asks. Change nothing else.

3. Finish. When the file is written: `loop-manager finish pass --summary "<what GLOSSARY.md holds>"`.
   If you cannot write it: `loop-manager finish fail --summary "<why>"`.

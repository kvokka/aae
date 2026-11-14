You work in `{{ rig.path }}`, the rig `{{ rig.name }}` of the project `{{ project.name }}`. Your task:

<task>
{{ inputs.task }}
</task>

Work through these steps.

1. Read the task to its end. It holds everything you get: follow its constraints exactly (names, files, ports, paths,
   formats, what not to use). Work only inside `{{ rig.path }}`.

2. Do the work.

3. Test it the way the task says it will be checked, for real: run it, call it, read what it produces. When you start
   a server or any other process to test, stop it afterwards. Leave nothing running and no test data behind.

4. Fix what the test shows and test again until it works.

5. Finish. When it works:
   `aae finish pass --summary "<what you made, in one sentence>" --data '<JSON object>'`, where the JSON holds what
   someone who uses your work needs to know (the files, how to start it, the addresses and paths it serves, the
   formats it takes) — `{}` if there is nothing.
   If you cannot make it work: `aae finish fail --summary "<what does not work and why>"`.

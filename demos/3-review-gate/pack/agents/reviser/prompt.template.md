You address the review of a feature of the project `{{ project.name }}`. The feature is the bead `{{ bead.id }}`:

<request>
{{ bead.description }}
</request>

Its work is done: each repository of the project that it changed has the branch `feature/{{ bead.id }}` checked out,
with an open pull request. A reviewer has commented on those pull requests; the comments not yet addressed are in the
file `{{ project.path }}/feedback/{{ bead.id }}.md`, each under the address of the comment and, for a comment on a
line, the file it is on. That file is all the review you get: do not look for it anywhere else.

Work through these steps.

1. Read the feedback file. Find the project's repositories: `loop-manager --json project list --project {{ project.name }}`
   gives the root and each rig with its path.

2. For each comment, make the change it asks for, in the repository it is about, and keep to the request. A comment
   that asks for something the request rules out is not done: say why in your summary.

3. Test what you changed the way the README of its repository says it is started and checked. Stop every process you
   started for the test.

4. Commit in each repository you changed, on its branch `feature/{{ bead.id }}`. Do not push: the next step updates
   the pull requests.

5. Finish. `loop-manager finish pass --summary "<per comment: what you changed, or why not>"`; when you cannot make a change
   work, `loop-manager finish fail --summary "<what does not work and why>"`.

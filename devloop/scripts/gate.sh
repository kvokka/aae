#!/bin/sh
# The step `gate` (formulas/epic.toml), a check with `back_to = "code"`. Runs in the rig, the root of the repository,
# with the epic in LOOP_MANAGER_BEAD, the reviewer's `finish --data` in LOOP_MANAGER_STEP_REVIEW_DATA and the result
# of the step `tests` in LOOP_MANAGER_STEP_TESTS_DATA. It commits the tracker and pushes, whatever the review says.
# Exit 0: `verdict` is `complete` and `score` is 9 or more. Exit 1: anything else; the epic gets the status `rework`
# and the run goes back to the coder with what this printed. Exit 75: the review passes and the push failed; the step
# is tried again.
set -eu
bead=${LOOP_MANAGER_BEAD:?}
field() { printf '%s' "${LOOP_MANAGER_STEP_REVIEW_DATA:-null}" | jq -r "$1" 2>/dev/null || true; }
score=$(field '.score? | tonumber?')
verdict=$(field '.verdict? | strings')
pass=$(field '(.verdict? == "complete") and ((.score? | tonumber?) >= 9)')

[ "$pass" = true ] || br update "$bead" -s rework -q || echo "gate: br did not set the status rework on $bead"
br sync --flush-only -q || echo "gate: br did not write devloop/.beads/issues.jsonl"
export GIT_TERMINAL_PROMPT=0   # a push that would ask for a password fails instead of waiting
git add -- devloop/.beads
git diff --cached --quiet -- devloop/.beads || git commit -q -m "devloop: the tracker of $bead" -- devloop/.beads
pushed=yes
git push -q || pushed=no

if [ "$pass" = true ]; then
  [ "$pushed" = yes ] || { echo "gate: the review passes, but git push failed"; exit 75; }
  echo "gate: SCORE $score/10, VERDICT $verdict"
  exit 0
fi
[ "$pushed" = yes ] || echo "gate: git push failed; the next round pushes again"
echo "gate: SCORE ${score:-none}/10, VERDICT ${verdict:-none}: the bar is 9 with \`complete\`. The review is the" \
  "reviewer's last comment on $bead; a round without one is reviewed again."
printf '%s' "${LOOP_MANAGER_STEP_TESTS_DATA:-null}" | jq -r '.stdout? // empty' 2>/dev/null | grep -E '^(RESULT|HEAD|LOG): ' || true
exit 1

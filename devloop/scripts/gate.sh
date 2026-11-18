#!/bin/sh
# The step `gate` (formulas/task.toml). Runs in the rig, the root of the repository, with the run's id in
# LOOP_MANAGER_RUN and its bead in LOOP_MANAGER_BEAD. It commits `.loop/` and the tracker and pushes, whatever the
# review says: a round sent back runs no step after this one. Then it judges `.loop/runs/<run>/review.md`.
# Exit 0: `SCORE` of 9 or more with `VERDICT: complete`. Exit 1: anything else; the run goes back to the coder with
# what this printed. Exit 75: the review passes and the push failed; the step is tried again.
set -eu
run=${LOOP_MANAGER_RUN:?}
review=".loop/runs/$run/review.md"
[ -f "$review" ] || { echo "gate: no review at $review"; exit 1; }
score=$(sed -n '1s/^SCORE: \(.*\)\/10$/\1/p' "$review")
verdict=$(sed -n '2s/^VERDICT: //p' "$review")

export GIT_TERMINAL_PROMPT=0   # a push that would ask for a password fails instead of waiting
git add -- .loop devloop/.beads
git diff --cached --quiet || git commit -q -m "devloop: review of ${LOOP_MANAGER_BEAD:-$run}, ${score:-no score}/10"
pushed=yes
git push -q || pushed=no

case "$verdict $score" in
  "complete 9" | "complete 9."[0-9] | "complete 10")
    [ "$pushed" = yes ] || { echo "gate: the review passes, but git push failed"; exit 75; }
    echo "gate: SCORE $score/10, VERDICT $verdict"
    ;;
  *)
    [ "$pushed" = yes ] || echo "gate: git push failed; the next round pushes again"
    echo "gate: SCORE ${score:-?}/10, VERDICT ${verdict:-?}: the bar is 9 with 'complete'. The review is $review:" \
      "its mandatory items are the first work of the next round"
    exit 1
    ;;
esac

#!/bin/sh
# Commit the round's tracker state locally. The close step publishes the completed epic.
set -eu
bead=${LOOP_MANAGER_BEAD:?}
head=$(git rev-parse HEAD)
ci=$(printf '%s\n' "${LOOP_MANAGER_STEP_CI_REPORT_DATA:-null}" | jq -ec '.stdout | fromjson' || echo null)
review=${LOOP_MANAGER_STEP_REVIEW_DATA:-null}
score=$(printf '%s\n' "$review" | jq -r '.score? | tonumber?' || true)
verdict=$(printf '%s\n' "$review" | jq -r '.verdict? | strings' || true)
changes=$(git status --porcelain -- . ':!devloop/.beads')
pass=false
if [ -z "$changes" ] && jq -en --argjson ci "$ci" --argjson review "$review" --arg head "$head" \
  '($ci.exit_code == 0 and $ci.uncommitted == 0 and $ci.head == $head)
   and ($review.verdict? == "complete" and ($review.score? | tonumber?) >= 9)' >/dev/null; then
  pass=true
fi
if [ "$pass" = false ]; then br update "$bead" -s rework -q; fi
br sync --flush-only -q
git add -- devloop/.beads
git diff --cached --quiet -- devloop/.beads || git commit -q -m "chore(devloop): record tracker state [$bead]" -- devloop/.beads

echo "gate: SCORE ${score:-none}/10, VERDICT ${verdict:-none}"
if [ "$pass" = true ]; then exit 0; fi
echo "Complete requires a score of 9 or higher and green CI on the current clean revision $head."
printf '%s\n' "$ci" | jq -r 'select(. != null) | "CI exit \(.exit_code) at \(.head), uncommitted: \(.uncommitted); LOG: \(.log)"'
exit 1

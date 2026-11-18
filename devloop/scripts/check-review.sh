#!/bin/sh
# The check of the step `review` (formulas/task.toml): the reviewer left this round's review with the two lines the
# gate reads, and changed nothing in the tree. Runs in the rig, the root of the repository, with the run's id in
# LOOP_MANAGER_RUN. Exit 0: passed; 1: not passed, and what it printed goes to the reviewer as a remark.
set -eu
dir=".loop/runs/${LOOP_MANAGER_RUN:?}"
review="$dir/review.md"
[ -f "$review" ] || { echo "check-review: no review: write $review"; exit 1; }
sed -n 1p "$review" | grep -Eq '^SCORE: (10|[0-9](\.[0-9])?)/10$' || {
  echo "check-review: line 1 of $review must be 'SCORE: <0 to 10, at most one decimal>/10', as 'SCORE: 8.5/10'"
  exit 1
}
sed -n 2p "$review" | grep -Eq '^VERDICT: (complete|continue)$' || {
  echo "check-review: line 2 of $review must be 'VERDICT: complete' or 'VERDICT: continue'"
  exit 1
}
# The review of an earlier round is older than the report the coder rewrote since.
[ -z "$(find "$dir/report.md" -newer "$review" 2>/dev/null)" ] || {
  echo "check-review: $review is older than the report: write the review of this round"
  exit 1
}
dirty=$(git status --porcelain -- . ':!.loop' ':!devloop/.beads')
[ -z "$dirty" ] || {
  echo "check-review: the tree has changes outside .loop/: a reviewer changes nothing, undo them (a mutation left in?)"
  echo "$dirty" | head -n 20
  exit 1
}

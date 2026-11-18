#!/bin/sh
# The check of the step `code` (formulas/task.toml): the coder left its report, with the commit the run started
# from (the reviewer takes the diff from it), and nothing outside `.loop/` and the tracker is uncommitted, since the
# next step runs `mise run ci` on HEAD. Runs in the rig, the root of the repository, with the run's id in
# LOOP_MANAGER_RUN. Exit 0: passed; 1: not passed, and what it printed goes to the coder as a remark.
set -eu
report=".loop/runs/${LOOP_MANAGER_RUN:?}/report.md"
[ -f "$report" ] || { echo "check-report: no report: write $report"; exit 1; }
sed -n 1p "$report" | grep -Eq '^STATUS: (done|partial)$' || {
  echo "check-report: line 1 of $report must be 'STATUS: done' or 'STATUS: partial';" \
    "a blocked task ends with 'loop-manager finish fail'"
  exit 1
}
base=$(sed -n '3s/^BASE: //p' "$report")
git merge-base --is-ancestor "${base:-none}" HEAD 2>/dev/null || {
  echo "check-report: line 3 of $report must be 'BASE: <commit>': the HEAD the run started from, an ancestor of HEAD"
  exit 1
}
dirty=$(git status --porcelain -- . ':!.loop' ':!devloop/.beads')
[ -z "$dirty" ] || {
  echo "check-report: uncommitted changes outside .loop/: commit or undo them, the CI gate runs on HEAD"
  echo "$dirty" | head -n 20
  exit 1
}

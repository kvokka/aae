#!/bin/sh
# Self-test of the scripts beside it, on sample files in a throwaway repository: each case names the exit code it
# expects. `mise run ci` runs it.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
export LOOP_MANAGER_RUN=run_1 LOOP_MANAGER_BEAD=lm-1
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null   # no hooks, signing or identity of the host
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.org
runs=.loop/runs/run_1

# A repository with a remote to push to, one commit, and the directories the scripts name.
fresh() {
  r=$(mktemp -d "$tmp/r.XXXXXX")
  git init -q --bare "$r/origin.git"
  git init -q "$r/work" && cd "$r/work"
  git remote add origin "$r/origin.git"
  mkdir -p $runs devloop/.beads src
  : >devloop/.beads/issues.jsonl
  echo one >src/a
  git add -A && git commit -q -m init && git push -q -u origin HEAD
}
expect() { # expect <exit code> <script> <label>
  got=0
  sh "$here/$2" >"$tmp/out" 2>&1 || got=$?
  [ "$got" = "$1" ] || { echo "selftest: $2: $3: expected exit $1, got $got"; cat "$tmp/out"; exit 1; }
}
report() { printf 'STATUS: %s\nBEAD: lm-1\nBASE: %s\n\n# A task\n' "$1" "${2:-$(git rev-parse HEAD)}" >$runs/report.md; }
review() { printf 'SCORE: %s\nVERDICT: %s\n\n# Review\n' "$1" "$2" >$runs/review.md; }

fresh; expect 1 check-report.sh "no report"
fresh; report 'done'; expect 0 check-report.sh "done"
fresh; report partial; expect 0 check-report.sh "partial"
fresh; report blocked; expect 1 check-report.sh "blocked"
fresh; printf '# A task\nSTATUS: done\n' >$runs/report.md; expect 1 check-report.sh "STATUS not on line 1"
fresh; report 'done' none; expect 1 check-report.sh "BASE is no commit"
fresh; printf 'STATUS: done\nBEAD: lm-1\n\n# A task\n' >$runs/report.md; expect 1 check-report.sh "no BASE line"
fresh; report 'done'; echo two >src/a; expect 1 check-report.sh "uncommitted change"
fresh; report 'done'; echo new >src/b; expect 1 check-report.sh "untracked file"
fresh; report 'done'; echo x >devloop/.beads/issues.jsonl; expect 0 check-report.sh "only the tracker changed"

fresh; report 'done'; expect 1 check-review.sh "no review"
fresh; report 'done'; review 9/10 complete; expect 0 check-review.sh "9 complete"
fresh; report 'done'; review 8.5/10 continue; expect 0 check-review.sh "8.5 continue"
fresh; report 'done'; review 10/10 complete; expect 0 check-review.sh "10"
fresh; report 'done'; review 11/10 complete; expect 1 check-review.sh "score over 10"
fresh; report 'done'; review 8.55/10 continue; expect 1 check-review.sh "two decimals"
fresh; report 'done'; review 9 complete; expect 1 check-review.sh "score without /10"
fresh; report 'done'; review 9/10 'done'; expect 1 check-review.sh "unknown verdict"
fresh; review 9/10 complete; touch -t 200001010000 $runs/review.md; report 'done'; expect 1 check-review.sh "review older than the report"
fresh; report 'done'; review 9/10 complete; echo two >src/a; expect 1 check-review.sh "tree changed"

pushed() { [ "$(git rev-parse HEAD)" = "$(git -C ../origin.git rev-parse HEAD)" ] || { echo "selftest: gate.sh: $1: not pushed"; exit 1; }; }
committed() { [ -z "$(git status --porcelain)" ] || { echo "selftest: gate.sh: $1: .loop/ not committed"; exit 1; }; }
fresh; expect 1 gate.sh "no review"
fresh; report 'done'; review 9/10 complete; expect 0 gate.sh "9 complete"; committed "9 complete"; pushed "9 complete"
fresh; report 'done'; review 9.5/10 complete; expect 0 gate.sh "9.5 complete"
fresh; report 'done'; review 10/10 complete; expect 0 gate.sh "10 complete"
fresh; report 'done'; review 8.9/10 complete; expect 1 gate.sh "8.9"; committed "8.9"; pushed "8.9"
fresh; report 'done'; review 9/10 continue; expect 1 gate.sh "9 continue"; pushed "9 continue"
fresh; report 'done'; review 9/10 complete; echo x >devloop/.beads/issues.jsonl; expect 0 gate.sh "tracker"; committed "tracker"
fresh; report 'done'; review 9/10 complete; git remote set-url origin "$r/gone.git"; expect 75 gate.sh "pass, push failed"
fresh; report 'done'; review 7/10 continue; git remote set-url origin "$r/gone.git"; expect 1 gate.sh "fail, push failed"; committed "fail, push failed"
echo "devloop selftest: ok"

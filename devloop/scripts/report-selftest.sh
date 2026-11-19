#!/bin/sh
# Real br, TOON and a local git remote exercise closure, extraction, bounded output and retry without a model.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
unset BEADS_DIR BEADS_DB BD_ACTOR BR_OUTPUT_FORMAT LOOP_MANAGER_SOCKET LOOP_MANAGER_ATTEMPT_TOKEN
export LOOP_MANAGER_RUN=run_report
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.org
git init -q --bare "$tmp/origin.git"
git init -q "$tmp/work"
cd "$tmp/work"
git remote add origin "$tmp/origin.git"
mkdir -p devloop target/devloop/runs/run_report/tests.first target/devloop/runs/run_report/notes
printf 'target/\ndevloop/artifacts/\n' >.gitignore
(cd devloop && br init --prefix dev -q)
export BEADS_DIR="$tmp/work/devloop/.beads"
bead=$(br create 'Report the epic' --json | jq -r .id)
export LOOP_MANAGER_BEAD=$bead
br update "$bead" --assignee loop-manager:run_report -q
comment() { br comments add "$bead" "$1" -q; }
comment 'CODER: done
BASE: abc
HEAD: def
The requested work is complete.'
comment 'QUESTION: q1
Which branch?'
comment 'DECISION: d1
Chose a shared checkout. Owner should know.'
comment 'ANSWER: q1
Use main.'
comment 'QUESTION: q2
An unresolved question.'
comment 'DECISION: d2
A decision that was accepted.'
comment 'ANSWER: d2
Accepted.'
comment 'REVIEW
SCORE: 9/10
VERDICT: complete
HEAD: def
The work is complete.'
printf 'HEAD: def\nRESULT: pass\n' >target/devloop/runs/run_report/tests.first/result.txt
echo 'full CI output' >target/devloop/runs/run_report/tests.first/ci.log
echo 'plan detail' >target/devloop/runs/run_report/notes/plan.md
git add -A && git commit -q -m init && git push -q -u origin HEAD
# Quiet git still forwards hook stdout. The final result must remain a standalone TOON document.
printf '#!/bin/sh\necho "pre-commit progress that is not part of the report"\n' >.git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
before=$(git rev-parse HEAD)
sh "$here/report.sh" snapshot run_report "$bead" >"$tmp/snapshot.toon"
toon --decode "$tmp/snapshot.toon" >"$tmp/snapshot.json"
jq -e '.status == "open" and .comments == null and .artifacts == "target/devloop/runs/run_report"
  and (.questions | length) == 1 and .questions[0].key == "q2"
  and (.decisions | length) == 1 and .decisions[0].key == "d1"' "$tmp/snapshot.json" >/dev/null
[ "$(git rev-parse HEAD)" = "$before" ]
[ ! -e devloop/results ]
# Finalization publishes both a closed bead and the portable report; full logs stay local.
sh "$here/report.sh" >"$tmp/result.toon"
toon --decode "$tmp/result.toon" >"$tmp/result.json"
jq -e '.status == "closed" and .question_count == 1 and .decision_count == 1
  and .report == "devloop/results/run_report/report.toon"' "$tmp/result.json" >/dev/null
toon --decode devloop/results/run_report/report.toon >"$tmp/report.json"
jq -e '.code_head == "def" and (.tests | length) == 1' "$tmp/report.json" >/dev/null
grep -q 'full CI output' devloop/artifacts/run_report/tests.first/ci.log
grep -q 'plan detail' devloop/artifacts/run_report/notes/plan.md
[ "$(git rev-parse HEAD)" = "$(git -C ../origin.git rev-parse HEAD)" ]
git -C ../origin.git show HEAD:devloop/.beads/issues.jsonl | jq -e 'select(.status == "closed")' >/dev/null
[ -z "$(git status --porcelain)" ]
before=$(git rev-parse HEAD)
sh "$here/report.sh" >"$tmp/retry.toon"
[ "$(git rev-parse HEAD)" = "$before" ]
cmp "$tmp/result.toon" "$tmp/retry.toon"
# A failed push leaves a retryable report; retry after restoring the remote does not duplicate commits.
comment 'QUESTION: q1
This question was reopened after the earlier answer.'
git remote set-url origin "$tmp/missing.git"
rc=0
sh "$here/report.sh" >"$tmp/failed.out" 2>&1 || rc=$?
[ "$rc" = 75 ]
before=$(git rev-parse HEAD)
git remote set-url origin "$tmp/origin.git"
sh "$here/report.sh" >"$tmp/retry.toon"
[ "$(git rev-parse HEAD)" = "$before" ]
toon --decode "$tmp/retry.toon" >"$tmp/retry.json"
jq -e '.question_count == 2' "$tmp/retry.json" >/dev/null
# Even large UTF-8 comments cannot truncate the exec result into an unreadable TOON fragment.
long=$(jq -nr '"🧭" * 4000')
comment "CODER: done
$long"
comment "REVIEW
HEAD: def
$long"
sh "$here/report.sh" >"$tmp/large.toon"
[ "$(wc -c <"$tmp/large.toon")" -le 3500 ]
toon --decode "$tmp/large.toon" >"$tmp/large.json"
jq -e '.report == "devloop/results/run_report/report.toon"' "$tmp/large.json" >/dev/null
echo "devloop report selftest: ok"

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
export LOOP_MANAGER_BEAD="$bead"
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
printf 'HEAD: def\nRESULT: pass\n' >target/devloop/runs/run_report/tests.first/result.txt
echo 'full CI output' >target/devloop/runs/run_report/tests.first/ci.log
echo 'plan detail' >target/devloop/runs/run_report/notes/plan.md
git add -A && git commit -q -m init && git push -q -u origin HEAD
# Quiet git still forwards hook stdout. The final result must remain a standalone TOON document.
printf '#!/bin/sh\necho "pre-commit progress that is not part of the report"\n' >.git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
before=$(git rev-parse HEAD)
# A run without a review HEAD first uses the checked-out revision.
sh "$here/report.sh" snapshot run_report "$bead" >"$tmp/snapshot.toon"
grep -Fxq "code_head: $before" "$tmp/snapshot.toon" || grep -Fxq "code_head: \"$before\"" "$tmp/snapshot.toon"
comment 'REVIEW
SCORE: 9/10
VERDICT: complete
HEAD: def
The work is complete.'
sh "$here/report.sh" snapshot run_report "$bead" >"$tmp/snapshot.toon"
br comments "$bead" --json >"$tmp/comments.json"
jq --arg bead "$bead" '
  def note($prefix): first(.[] | select(.text | startswith($prefix)));
  {run: "run_report", epic: $bead, title: "Report the epic", status: "open", comments: null,
   code_head: "def", artifacts: "target/devloop/runs/run_report", tests: ["HEAD: def\nRESULT: pass\n"],
   coder: note("CODER:").text, review: note("REVIEW\n").text,
   questions: [note("QUESTION: q2\n") | {author, created_at, text, key: "q2"}],
   decisions: [note("DECISION: d1\n") | {author, created_at, text, key: "d1"}]}' \
  "$tmp/comments.json" >"$tmp/snapshot.json"
loop-manager format <"$tmp/snapshot.json" >"$tmp/expected.toon"
cmp "$tmp/expected.toon" "$tmp/snapshot.toon"
[ "$(git rev-parse HEAD)" = "$before" ]
[ ! -e devloop/results ]
# Finalization publishes both a closed bead and the portable report; full logs stay local.
sh "$here/report.sh" >"$tmp/result.toon"
archive=devloop/results/run_report
jq --arg comments "$archive/comments.toon" '.status = "closed" | .comments = $comments
  | .artifacts = "devloop/artifacts/run_report"' "$tmp/snapshot.json" >"$tmp/closed.json"
loop-manager format <"$tmp/closed.json" >"$tmp/expected.toon"
cmp "$tmp/expected.toon" "$archive/report.toon"
loop-manager format <"$tmp/comments.json" >"$tmp/expected.toon"
cmp "$tmp/expected.toon" "$archive/comments.toon"
jq --arg report "$archive/report.toon" '{run, epic, status, report: $report, artifacts,
  summary: .coder, review, question_count: 1, decision_count: 1,
  questions: [.questions[] | {key, text}], decisions: [.decisions[] | {key, text}]}' \
  "$tmp/closed.json" >"$tmp/preview.json"
loop-manager format <"$tmp/preview.json" >"$tmp/expected.toon"
cmp "$tmp/expected.toon" "$tmp/result.toon"
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
grep -Fxq 'question_count: 2' "$tmp/retry.toon"
# Even large UTF-8 comments cannot truncate the exec result into an unreadable TOON fragment.
long=$(jq -nr '"🧭" * 4000')
comment "CODER: done
$long"
comment "REVIEW
HEAD: def
$long"
sh "$here/report.sh" >"$tmp/large.toon"
[ "$(wc -c <"$tmp/large.toon")" -le 3500 ]
grep -Fxq "report: $archive/report.toon" "$tmp/large.toon"
# Retry an old handoff with no review HEAD. Its archived revision, including a quoted
# numeric-looking SHA, survives both the first native write and subsequent retries.
export LOOP_MANAGER_RUN=legacy
LOOP_MANAGER_BEAD=$(br create 'Legacy handoff' --json | jq -r .id)
mkdir -p devloop/results/legacy
printf '%s\n' 'status: closed' 'code_head: "1234567"' 'questions[0]:' >devloop/results/legacy/report.toon
sh "$here/report.sh" >"$tmp/legacy.toon"
grep -Fxq 'code_head: "1234567"' devloop/results/legacy/report.toon
before=$(git rev-parse HEAD)
sh "$here/report.sh" >"$tmp/retry.toon"
[ "$(git rev-parse HEAD)" = "$before" ]
cmp "$tmp/legacy.toon" "$tmp/retry.toon"
echo "devloop report selftest: ok"

#!/bin/sh
# Exercise finalization with real br and a private git remote.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
unset BEADS_DIR BEADS_DB BD_ACTOR BR_OUTPUT_FORMAT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.org
export LOOP_MANAGER_RUN=run_test
git init -q --bare "$tmp/origin.git"
git init -q "$tmp/work"
cd "$tmp/work"
git remote add origin "$tmp/origin.git"
mkdir -p devloop target/devloop/runs/run_test
printf 'target/\ndevloop/artifacts/\n' >.gitignore
echo 'full CI log' >target/devloop/runs/run_test/ci.log
(cd devloop && br init --prefix dev -q)
export BEADS_DIR="$PWD/devloop/.beads"
bead=$(br create 'Close the epic' --json | jq -er .id)
export LOOP_MANAGER_BEAD="$bead"
br update "$bead" --assignee loop-manager:run_test -q
br comments add "$bead" 'CODER: done' -q
git add -A && git commit -qm init && git push -qu origin HEAD

git remote set-url origin "$tmp/missing.git"
rc=0
sh "$here/close.sh" >"$tmp/output" 2>&1 || rc=$?
[ "$rc" = 75 ]
before=$(git rev-parse HEAD)
git remote set-url origin "$tmp/origin.git"
sh "$here/close.sh"
[ "$(git rev-parse HEAD)" = "$before" ]
[ "$(git -C "$tmp/origin.git" rev-parse HEAD)" = "$before" ]
git -C "$tmp/origin.git" show HEAD:devloop/.beads/issues.jsonl |
  jq -e '.status == "closed" and .assignee == "loop-manager:run_test" and .comments[0].text == "CODER: done"' >/dev/null
cmp target/devloop/runs/run_test/ci.log devloop/artifacts/run_test/ci.log
[ ! -e devloop/results ]
[ -z "$(git status --porcelain)" ]
echo "devloop close selftest: ok"

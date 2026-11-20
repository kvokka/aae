#!/bin/sh
# Self-test of the scripts beside it, in a throwaway repository with a remote, with `br` and `mise` replaced by stubs:
# each case names the exit code it expects. `mise run ci` runs it, also inside a run of this pack: nothing of the
# run's environment reaches the scripts.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
unset BEADS_DIR BD_ACTOR LOOP_MANAGER_STEP_REVIEW_DATA LOOP_MANAGER_STEP_TESTS_DATA
export LOOP_MANAGER_RUN=run_1 LOOP_MANAGER_BEAD=dev-1
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null   # no hooks, signing or identity of the host
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.org

# `br` writes its arguments to a log and, on a write, touches the tracker file; `mise` exits as MISE_RC says.
mkdir "$tmp/bin"
printf '#!/bin/sh\necho "$*" >>"%s/br.log"\necho "$*" >>devloop/.beads/issues.jsonl\n' "$tmp" >"$tmp/bin/br"
printf '#!/bin/sh\necho "ci says $*"\nexit "${MISE_RC:-0}"\n' >"$tmp/bin/mise"
chmod +x "$tmp/bin/br" "$tmp/bin/mise"
export PATH="$tmp/bin:$PATH"

# A repository with a remote to push to, one commit, and the tracker's directory.
fresh() {
  r=$(mktemp -d "$tmp/r.XXXXXX")
  : >"$tmp/br.log"
  git init -q --bare "$r/origin.git"
  git init -q "$r/work" && cd "$r/work"
  git remote add origin "$r/origin.git"
  mkdir -p devloop/.beads src
  : >devloop/.beads/issues.jsonl
  echo one >src/a
  printf 'target/\n' >.gitignore
  git add -A && git commit -q -m init && git push -q -u origin HEAD
}
expect() { # expect <exit code> <script> <label>
  got=0
  sh "$here/$2" >"$tmp/out" 2>&1 || got=$?
  [ "$got" = "$1" ] || { echo "selftest: $2: $3: expected exit $1, got $got"; cat "$tmp/out"; exit 1; }
}
said() { grep -q -- "$1" "$tmp/out" || { echo "selftest: $2: the output lacks '$1'"; cat "$tmp/out"; exit 1; }; }
review() { export LOOP_MANAGER_STEP_REVIEW_DATA="$1"; }
pushed() { [ "$(git rev-parse HEAD)" = "$(git -C ../origin.git rev-parse HEAD)" ] || { echo "selftest: gate.sh: $1: not pushed"; exit 1; }; }
rework() { grep -q "^update dev-1 -s rework" "$tmp/br.log" || { echo "selftest: gate.sh: $1: no status rework"; exit 1; }; }
no_rework() { ! grep -q rework "$tmp/br.log" || { echo "selftest: gate.sh: $1: status rework on a pass"; exit 1; }; }

fresh; review '{"score":9,"verdict":"complete"}'; expect 0 gate.sh "9 complete"; pushed "9 complete"; no_rework "9 complete"
fresh; review '{"score":9.5,"verdict":"complete"}'; expect 0 gate.sh "9.5 complete"
fresh; review '{"score":"10","verdict":"complete"}'; expect 0 gate.sh "the score as text"
fresh; review '{"score":8.9,"verdict":"complete"}'; expect 1 gate.sh "8.9"; rework "8.9"; pushed "8.9"
fresh; review '{"score":9,"verdict":"continue"}'; expect 1 gate.sh "9 continue"; rework "9 continue"
fresh; review '{"verdict":"complete"}'; expect 1 gate.sh "no score"
fresh; review '"SCORE 9/10"'; expect 1 gate.sh "data that is no object"; said "VERDICT none" "no object"
fresh; unset LOOP_MANAGER_STEP_REVIEW_DATA; expect 1 gate.sh "no data"; said "VERDICT none" "no data"; rework "no data"
# The tracker the stub wrote is committed, and nothing else of the tree is.
fresh; review '{"score":7,"verdict":"continue"}'; echo two >src/a; echo new >src/b; expect 1 gate.sh "dirty tree"; pushed "dirty tree"
[ "$(git status --porcelain | sort | tr -d '\n')" = " M src/a?? src/b" ] || { echo "selftest: gate.sh: dirty tree: the gate touched the coder's tree"; git status --porcelain; exit 1; }
fresh; review '{"score":7,"verdict":"continue"}'; export LOOP_MANAGER_STEP_TESTS_DATA='{"exit_code":0,"stdout":"HEAD: abc\nRESULT: FAIL (mise run ci: exit 101)\n","stderr":""}'
expect 1 gate.sh "tests in the remark"; said "RESULT: FAIL" "tests in the remark"; unset LOOP_MANAGER_STEP_TESTS_DATA
fresh; review '{"score":9,"verdict":"complete"}'; git remote set-url origin "$r/gone.git"; expect 75 gate.sh "pass, push failed"
fresh; review '{"score":7,"verdict":"continue"}'; git remote set-url origin "$r/gone.git"; expect 1 gate.sh "fail, push failed"

fresh; MISE_RC=0 expect 0 tests.sh "green"; said "RESULT: pass" "green"; said "UNCOMMITTED: 0 paths" "green"
grep -q "ci says run ci" target/devloop/ci.log || { echo "selftest: tests.sh: no log"; exit 1; }
fresh; echo two >src/a; export MISE_RC=101; expect 0 tests.sh "red"; unset MISE_RC
said "RESULT: FAIL (mise run ci: exit 101)" "red"; said "ci says run ci" "red"; said "UNCOMMITTED: 1 paths" "red"
echo "devloop selftest: ok"

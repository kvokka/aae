#!/bin/sh
# Self-test of the scripts beside it, in a throwaway repository with a remote, with `br` and `mise` replaced by stubs:
# each case names the exit code it expects. `mise run ci` runs it, also inside a run of this pack: nothing of the
# run's environment reaches the scripts.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
sh "$here/owner-selftest.sh"
sh "$here/close-selftest.sh"
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
unset BEADS_DIR BD_ACTOR LOOP_MANAGER_STEP_REVIEW_DATA LOOP_MANAGER_STEP_CI_REPORT_DATA
export LOOP_MANAGER_RUN=run_1 LOOP_MANAGER_BEAD=dev-1
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null   # no hooks, signing or identity of the host
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.org

# `br` writes its arguments to a log and, on a write, touches the tracker file; `mise` exits as MISE_RC says.
mkdir "$tmp/bin"
printf '#!/bin/sh\necho "$*" >>"%s/br.log"\necho "$*" >>devloop/.beads/issues.jsonl\n' "$tmp" >"$tmp/bin/br"
cat >"$tmp/bin/mise" <<'STUB'
#!/bin/sh
# An inherited socket wins over XDG paths. An inherited tracker wins over the working directory.
test -z "${LOOP_MANAGER_RUN+x}${LOOP_MANAGER_SOCKET+x}${LOOP_MANAGER_ATTEMPT_TOKEN+x}${BD_ACTOR+x}" || exit 88
test -z "${DCG_CUSTOM_PATHS+x}" || exit 88
test -z "${GIT_CONFIG_COUNT+x}${GIT_CONFIG_KEY_0+x}${GIT_CONFIG_VALUE_0+x}${GIT_CONFIG_PARAMETERS+x}" || exit 88
test "$BEADS_DIR" != "$outer_tracker" && test "$XDG_STATE_HOME" != "$outer_state" || exit 88
test -d "$XDG_RUNTIME_DIR" && test -d "$XDG_STATE_HOME" || exit 88
echo "ci says $*"
rc=${MISE_RC:-0}
status=passed
[ "$rc" = 0 ] || status=failed
jq -n --arg status "$status" --argjson rc "$rc" \
  '{tasks:[{name:"ci:fmt",status:$status,exit_code:$rc},{name:"ci:last",status:"passed",exit_code:0}]}' >"$DEVLOOP_CI_REPORT"
exit "$rc"
STUB
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
  remote_head=$(git rev-parse HEAD)
  ci_data 0
}
ci_data() {
  LOOP_MANAGER_STEP_CI_REPORT_DATA=$(jq -cn --argjson rc "$1" --arg sha "${2:-$(git rev-parse HEAD)}" \
    '{stdout:({exit_code:$rc,head:$sha,uncommitted:0,log:"ci.log",failed:[]} | tojson)}')
  export LOOP_MANAGER_STEP_CI_REPORT_DATA
}
expect() { # expect <exit code> <script> <label>
  got=0
  sh "$here/$2" >"$tmp/out" 2>&1 || got=$?
  [ "$got" = "$1" ] || { echo "selftest: $2: $3: expected exit $1, got $got"; cat "$tmp/out"; exit 1; }
}
said() { grep -q -- "$1" "$tmp/out" || { echo "selftest: $2: the output lacks '$1'"; cat "$tmp/out"; exit 1; }; }
review() { export LOOP_MANAGER_STEP_REVIEW_DATA="$1"; }
local_only() {
  [ "$(git -C ../origin.git rev-parse HEAD)" = "$remote_head" ] &&
    [ "$(git rev-parse HEAD)" != "$remote_head" ] || { echo "selftest: expected a local tracker commit"; exit 1; }
}
rework() { grep -q "^update dev-1 -s rework" "$tmp/br.log" || { echo "selftest: gate.sh: $1: no status rework"; exit 1; }; }
no_rework() { ! grep -q rework "$tmp/br.log" || { echo "selftest: gate.sh: $1: status rework on a pass"; exit 1; }; }

fresh; review '{"score":9,"verdict":"complete"}'; expect 0 gate.sh "9 complete"; local_only; no_rework "9 complete"
fresh; review '{"score":9.5,"verdict":"complete"}'; expect 0 gate.sh "9.5 complete"
fresh; review '{"score":"10","verdict":"complete"}'; expect 0 gate.sh "the score as text"
fresh; review '{"score":8.9,"verdict":"complete"}'; expect 1 gate.sh "8.9"; rework "8.9"; local_only
fresh; review '{"score":9,"verdict":"continue"}'; expect 1 gate.sh "9 continue"; rework "9 continue"
fresh; review '{"verdict":"complete"}'; expect 1 gate.sh "no score"
fresh; review '"SCORE 9/10"'; expect 1 gate.sh "data that is no object"; said "VERDICT none" "no object"
fresh; unset LOOP_MANAGER_STEP_REVIEW_DATA; expect 1 gate.sh "no data"; said "VERDICT none" "no data"; rework "no data"
# The tracker the stub wrote is committed, and nothing else of the tree is.
fresh; review '{"score":7,"verdict":"continue"}'; echo two >src/a; echo new >src/b; expect 1 gate.sh "dirty tree"; local_only
[ "$(git status --porcelain | sort | tr -d '\n')" = " M src/a?? src/b" ] || { echo "selftest: gate.sh: dirty tree: the gate touched the coder's tree"; git status --porcelain; exit 1; }
fresh; review '{"score":9,"verdict":"complete"}'; ci_data 101; expect 1 gate.sh "red CI overrides acceptance"; said "CI exit 101" "red"
fresh; review '{"score":9,"verdict":"complete"}'; ci_data 0 deadbeef; expect 1 gate.sh "stale CI"; rework "stale CI"
fresh; review '{"score":9,"verdict":"complete"}'; echo dirty >>src/a; expect 1 gate.sh "review left a dirty tree"
fresh; review '{"score":9,"verdict":"complete"}'; git remote set-url origin "$r/gone.git"; expect 0 gate.sh "close step owns the push"

fresh
outer_state=$tmp/working-state
outer_tracker=$r/work/devloop/.beads
export outer_state outer_tracker
export BEADS_DIR="$outer_tracker" XDG_STATE_HOME="$outer_state" LOOP_MANAGER_SOCKET="$tmp/working.sock"
export BD_ACTOR=working LOOP_MANAGER_ATTEMPT_TOKEN=outer-token
for operator in "$here/../lm" "$here/install.sh"; do
  if "$operator" --help >"$tmp/operator" 2>&1; then echo "agent ran $operator" >&2; exit 1; fi
  grep -q 'operator command' "$tmp/operator"
  if "$here/test-env" "$operator" --help >"$tmp/operator" 2>&1; then echo "test ran $operator" >&2; exit 1; fi
  grep -q 'operator command' "$tmp/operator"
done
export DCG_CUSTOM_PATHS="$here/../dcg.yaml"
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=user.name GIT_CONFIG_VALUE_0=test
MISE_RC=0 expect 0 tests.sh "green"; jq -e '.exit_code == 0 and .uncommitted == 0 and .failed == []' "$tmp/out" >/dev/null
log=$(jq -r .log "$tmp/out")
grep -q "ci says run ci" "$log"
MISE_RC=0 expect 0 tests.sh "second test run"
[ "$(find target/devloop/runs/run_1 -name ci.log | wc -l | tr -d ' ')" = 2 ]
fresh; echo two >src/a; MISE_RC=101 expect 0 tests.sh "red report"
jq -e '.exit_code == 101 and .uncommitted == 1 and .failed == [{"name":"ci:fmt","status":"failed","exit_code":101}]' "$tmp/out" >/dev/null
log=$(jq -r .log "$tmp/out")
grep -Fq 'RESULT: FAIL (mise run ci: exit 101)' "${log%/*}/result.txt"
LOOP_MANAGER_STEP_CI_REPORT_DATA=$(jq -Rs '{stdout:.}' "$tmp/out")
unset LOOP_MANAGER_REMARKS
for n in 1 2 3; do
  expect 1 ci-route.sh "red CI returns to the coder"
  said "CI return $n/3" "return count"; said "ci:fmt" "failed check"
  LOOP_MANAGER_REMARKS="ci_route: exit 1
$(cat "$tmp/out")"
  export LOOP_MANAGER_REMARKS
done
expect 0 ci-route.sh "review the fourth failure"
said "sending the report to review" "escalation"
LOOP_MANAGER_REMARKS='gate: exit 1'
expect 1 ci-route.sh "new streak after review"; said "CI return 1/3" "reset"
ci_data 0; expect 0 ci-route.sh "green goes straight to review"
echo "devloop selftest: ok"

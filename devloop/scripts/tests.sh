#!/bin/sh
# Collect one CI invocation. The next step routes its exit code and the report is archived even on failure.
set -eu
run=${LOOP_MANAGER_RUN:?}
case "$run" in *[!a-zA-Z0-9_-]*) echo "tests: invalid run id" >&2; exit 1;; esac
mkdir -p "target/devloop/runs/$run"
dir=$(mktemp -d "target/devloop/runs/$run/tests.$(date -u +%Y%m%dT%H%M%SZ).XXXXXX")
log=$dir/ci.log
head=$(git rev-parse HEAD)
start=$(date +%s)
rc=0
DEVLOOP_CI_REPORT="$dir/checks.json" "$(dirname "$0")/test-env" mise run ci >"$log" 2>&1 || rc=$?
duration=$(($(date +%s) - start))
changes=$(git status --porcelain -- . ':!devloop/.beads')
uncommitted=0
if [ -n "$changes" ]; then uncommitted=$(printf '%s\n' "$changes" | wc -l); fi
failed='[]'
if [ -s "$dir/checks.json" ]; then
  failed=$(jq -ce '[.tasks[] | select(.status != "passed" and .status != "cached")
    | {name, status, exit_code}]' "$dir/checks.json")
elif [ "$rc" = 0 ]; then
  echo "tests: CI succeeded without writing $dir/checks.json" >&2
  exit 75
fi
{
  echo "HEAD: $head"
  echo "UNCOMMITTED: $uncommitted paths"
  echo "TIME: $duration s"
  echo "LOG: $log"
  printf '%s\n' "$failed" | jq -r '.[] | "CHECK: \(.name) \(.status) (exit \(.exit_code))"'
  if [ "$rc" = 0 ]; then echo "RESULT: pass"; else echo "RESULT: FAIL (mise run ci: exit $rc)"; fi
} >"$dir/result.txt"
jq -cn --argjson exit_code "$rc" --arg head "$head" --argjson uncommitted "$uncommitted" \
  --arg log "$log" --argjson failed "$failed" '{ $exit_code, $head, $uncommitted, $log, $failed }'

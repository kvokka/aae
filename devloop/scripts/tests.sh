#!/bin/sh
# The step `tests` (formulas/epic.toml): the gate of the repository, `mise run ci`, on the tree the coder left. Runs
# in the rig, the root of the repository. It is no check: it exits 0 red or green, and the last 4 KiB of what it
# prints are in the reviewer's prompt. The whole log is target/devloop/ci.log.
set -u
log=target/devloop/ci.log
mkdir -p target/devloop
start=$(date +%s)
rc=0
mise run ci >"$log" 2>&1 || rc=$?
if [ "$rc" != 0 ]; then
  echo "The end of $log:"
  tail -n 25 "$log" | cut -c1-120
fi
echo "HEAD: $(git rev-parse HEAD)"
echo "UNCOMMITTED: $(git status --porcelain -- . ':!devloop/.beads' | wc -l | tr -d ' ') paths"
echo "TIME: $(($(date +%s) - start)) s"
echo "LOG: $log"
if [ "$rc" = 0 ]; then echo "RESULT: pass"; else echo "RESULT: FAIL (mise run ci: exit $rc)"; fi

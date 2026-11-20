#!/usr/bin/env bash
# Check the MEASURE markers described in docs/agents/rules-code.md.
# Usage: scripts/measure.sh [repo-root]
set -euo pipefail
cd "${1:-$(git rev-parse --show-toplevel)}"

fail=0
err() { echo "measure: $*" >&2; fail=1; }

# Every MEASURE marker carries non-empty id, date, ver and env. `id=<...>` is the format template, not a marker; what
# the agents of a demo wrote (`demos/*/*-example/`) holds no markers of this repository.
rc=0
markers=$(git -c submodule.recurse=false grep -nE --untracked '(<!--|//|#) MEASURE id=' -- . \
  ':!.loop' ':!.claude' ':!scripts/measure.sh' ':!scripts/measure-selftest.sh' ':!demos/*-example/*') || rc=$?
((rc <= 1)) || err "git grep failed with code $rc"
while IFS= read -r line; do
  [[ -z "$line" || "$line" =~ MEASURE\ id=\< ]] && continue
  bare=$(sed 's/"[^"]*"//g' <<<"$line") # keys inside quoted values (note="... ver=1") do not count
  for key in id ver env; do
    [[ "$bare" =~ [[:space:]]$key=[^[:space:]\"] ]] || err "MEASURE marker without $key: $line"
  done
  [[ "$bare" =~ [[:space:]]date=[0-9]{4}-[0-9]{2}-[0-9]{2} ]] || err "MEASURE marker without date: $line"
done <<<"$markers"

exit "$fail"

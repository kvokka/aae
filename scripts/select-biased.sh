#!/usr/bin/env bash
# Every `select!` of `aae-core` polls its branches in a fixed order (`biased;`), with a comment that says why that
# order: a random pick among ready branches breaks the replay of a seed. Checks the code of `aae-core` outside its
# tests: the line after `select! {` must be a comment, and the first line after the comments `biased;`.
set -euo pipefail
cd "$(dirname "$0")/.."
found=$(find crates/aae-core/src -name '*.rs' -not -path '*/src/tests/*' -print0 | sort -z |
  xargs -0 awk '
    /select! *\{/ { at = FILENAME ":" FNR ": " $0; comments = 0; next }
    at && /^[[:space:]]*\/\// { comments++; next }
    at { if (!comments || $0 !~ /^[[:space:]]*biased;/) print at; at = "" }' || true)
if [[ -n $found ]]; then
  echo "select! in aae-core without a comment and \`biased;\` (a seed must replay):"
  echo "$found"
  exit 1
fi

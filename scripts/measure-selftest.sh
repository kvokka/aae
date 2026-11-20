#!/usr/bin/env bash
# Exercise the marker check on untracked files in an isolated repository.
set -euo pipefail
script="$(cd "$(dirname "$0")" && pwd)/measure.sh"
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
cd "$tmp"
git init -q

echo '<!-- MEASURE id=x date=2026-11-12 ver=0.1.0 env=linux note="n" -->' >notes.md
"$script" .
for misplaced in '' 'note="see ver=1"'; do
  echo "// MEASURE id=y date=2026-11-12 env=linux $misplaced" >m.rs
  if "$script" . >/dev/null 2>&1; then
    echo "measure-selftest: accepted a marker without ver" >&2
    exit 1
  fi
done
echo "measure-selftest: ok"

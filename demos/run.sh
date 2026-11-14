#!/bin/sh
# Runs one demo from two empty rigs to its smoke test with a daemon of its own; on a pass it keeps the rigs and
# RESULT.md in demos/<demo>/ as the snapshot. Usage: run.sh snake-claude|snake-mixed; AAE names the aae binary.
set -eu
demo=$1
here=$(cd "$(dirname "$0")" && pwd)
aae=$(command -v "${AAE:-aae}") || { echo "no aae: install it or set AAE"; exit 1; }
work=$(mktemp -d)
cp -R "$here/snake" "$work/snake"
mkdir -p "$work/$demo/backend" "$work/$demo/frontend"
cp -R "$here/$demo/aae.toml" "$here/$demo/agents" "$work/$demo/"
printf 'projects = ["%s"]\n[harness.claude]\n[harness.codex]\n' "$demo" >"$work/config.toml"

export XDG_STATE_HOME="$work/state" AAE_SOCKET="$work/aae.sock"
# The agents call `aae finish`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$aae"):$PATH" "$aae" daemon --config "$work/config.toml" 2>"$work/daemon.log" &
daemon=$!
trap 'kill $daemon 2>/dev/null || true' EXIT
until "$aae" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat "$work/daemon.log"; exit 1; }
  sleep 0.2
done
started=$(date +%s)
"$aae" run start "$demo" backend snake.backend

# Each trigger fires in the transaction of the run it follows, so once every run is terminal the chain is over.
result=timeout
while [ $(($(date +%s) - started)) -lt 1080 ]; do
  runs=$("$aae" --json run list --project "$demo")
  if printf "%s" "$runs" | jq -e 'any(.[]; .formula == "snake.smoke" and .state == "passed")' >/dev/null; then
    result=passed
    break
  fi
  if printf "%s" "$runs" | jq -e 'all(.[]; .state == "passed" or .state == "failed" or .state == "cancelled")' >/dev/null; then
    result=stopped
    break
  fi
  sleep 5
done
seconds=$(($(date +%s) - started))
versions="aae-$("$aae" --version | cut -d' ' -f2)"
for harness in claude codex; do
  versions="$versions,$harness-$("$harness" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
done
{
  echo "# $demo: $result"
  echo
  # Split so that scripts/budget.sh does not take this template for a marker.
  echo "<!-- MEASURE" "id=demo-$demo date=$(date -u +%Y-%m-%d) ver=$versions env=$(uname -sm | tr ' ' '-') src=demos/run.sh note=\"$result in $seconds s from the backend start, n=1\" -->"
  echo
  echo "The runs of the chain, newest first (\`aae run list\`):"
  echo
  echo '```'
  "$aae" run list --project "$demo"
  echo '```'
} >"$work/RESULT.md"
"$aae" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

echo "$demo: $result in ${seconds}s; the daemon's log and state stay in $work"
[ "$result" = passed ] || exit 1
snapshot="$here/$demo"
rm -rf "$snapshot/backend" "$snapshot/frontend"
cp -R "$work/$demo/backend" "$work/$demo/frontend" "$work/RESULT.md" "$snapshot/"
rm -f "$snapshot/backend/scores.txt" # what the smoke test posted

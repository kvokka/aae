#!/bin/sh
# Runs one demo from its request to the user's smoke test with a daemon of its own, on copies of the pack, the root
# and two empty rigs; on a pass it keeps the rigs and RESULT.md in demos/<demo>/ as the snapshot.
# Usage: run.sh snake-claude|snake-mixed; AAE names the aae binary.
set -eu
demo=$1
here=$(cd "$(dirname "$0")" && pwd)
aae=$(command -v "${AAE:-aae}") || { echo "no aae: install it or set AAE"; exit 1; }
work=$(mktemp -d)
cp -R "$here/dev-pack" "$work/dev-pack"
mkdir -p "$work/$demo/backend" "$work/$demo/frontend"
cp -R "$here/$demo/root" "$work/$demo/root"
root=$work/$demo/root

export XDG_STATE_HOME="$work/state" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export AAE_SOCKET="$work/aae.sock"
# The agents call `aae`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$aae"):$PATH" "$aae" daemon --config "$root/config.toml" 2>"$work/daemon.log" &
daemon=$!
trap 'kill $daemon 2>/dev/null || true' EXIT
until "$aae" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat "$work/daemon.log"; exit 1; }
  sleep 0.2
done
started=$(date +%s)
plan=$("$aae" run start "$demo" root dev.plan --input request="$(cat "$root/REQUEST.md")")

# The planner's pass starts the smoke run in the same transaction, so once the plan is over the smoke run exists.
result=passed
"$aae" run wait "$plan" --timeout 40m >"$work/plan.txt" || result=plan-$?
if [ "$result" = passed ]; then
  smoke=$("$aae" --json run list --project "$demo" | jq -r 'map(select(.formula | endswith(".smoke")))[0].id')
  "$aae" run wait "$smoke" --timeout 3m >"$work/smoke.txt" || result=smoke-$?
fi
seconds=$(($(date +%s) - started))
sha=$(git -C "$here" describe --always --dirty 2>/dev/null || echo unknown)
versions="aae-$("$aae" --version | cut -d' ' -f2)@$sha"
for harness in claude codex; do
  versions="$versions,$harness-$("$harness" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
done
runs=$("$aae" --json run list --project "$demo")
# The planner's own runs: its children but the smoke run, which its pass fired through the trigger.
children=$(printf "%s" "$runs" | jq --arg plan "$plan" \
  '[.[] | select(.parent_run == $plan and (.formula | endswith(".smoke") | not))] | length')
tokens=$(jq -rs '[.[] | select(.type == "run.passed" or .type == "run.failed") | .data.usage // empty]
  | "input \(map(.input_tokens) | add // 0), output \(map(.output_tokens) | add // 0)"' \
  "$XDG_STATE_HOME/aae/journal/events.jsonl")
{
  echo "# $demo: $result"
  echo
  # Split so that scripts/budget.sh does not take this template for a marker.
  echo "<!-- MEASURE" "id=demo-$demo date=$(date -u +%Y-%m-%d) ver=$versions env=$(uname -sm | tr ' ' '-')" \
    "src=demos/run.sh note=\"$result in $seconds s from the request, $children runs started by the planner, n=1\" -->"
  echo
  echo "Run on $(date -u +%Y-%m-%d) with $versions (\`$(uname -sm)\`): $result in $seconds s from the request."
  echo "Tokens the harnesses reported for the runs: $tokens."
  echo
  echo "The runs, oldest first, with the run that started each:"
  echo
  echo '| run | formula | rig | agent | state | parent |'
  echo '|---|---|---|---|---|---|'
  printf "%s" "$runs" | jq -r 'sort_by(.created_at) | .[] |
    "| \(.id) | \(.formula) | \(.workdir | split("/") | last) | \(.agent // "") | \(.state) | \(.parent_run // "") |"'
  echo
  echo "The planner's outcome (\`aae run wait\`):"
  echo
  echo '```'
  cat "$work/plan.txt"
  echo '```'
} >"$work/RESULT.md"
"$aae" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

echo "$demo: $result in ${seconds}s; the daemon's log and state stay in $work"
[ "$result" = passed ] || exit 1
snapshot="$here/$demo"
find "$snapshot/backend" "$snapshot/frontend" -delete 2>/dev/null || true
cp -R "$work/$demo/backend" "$work/$demo/frontend" "$work/RESULT.md" "$snapshot/"
rm -f "$snapshot/backend/scores.txt" # what the smoke test posted

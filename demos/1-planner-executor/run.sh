#!/bin/sh
# Runs the demo from the request to its report on a daemon of the demo's own. What the run creates stays in this
# directory and in .gitignore: the coders' work in frontend/ and backend/, the beads in root/.beads, the daemon's
# state in .loop-manager/ and RESULT.md. EXAMPLE=1 also keeps the result as the author's frontend-example/, backend-example/
# and RESULT-example.md. LOOP_MANAGER names the loop-manager binary.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
demo=$(basename "$here")
loop_manager=$(command -v "${LOOP_MANAGER:-loop-manager}") || { echo "no loop-manager: install it or set LOOP_MANAGER"; exit 1; }
request="Create me snake game in browser and a simple backend which store the high-scores, KISS"
cd "$here"
if [ -n "$(ls -A frontend backend root/.beads .loop-manager 2>/dev/null | grep -v -e '^\.gitkeep$' -e '^\.git$' -e ':$' -e '^$')" ]; then
  echo "$demo: a run is left over; start clean with: git clean -ffdX -- $here"
  exit 1
fi

mkdir -p .loop-manager
export XDG_STATE_HOME="$here/.loop-manager/state" XDG_CONFIG_HOME="$here/.loop-manager/config" XDG_CACHE_HOME="$here/.loop-manager/cache"
# A socket path has a length limit: it goes to a short temporary directory.
LOOP_MANAGER_SOCKET="$(mktemp -d)/loop-manager.sock"
export LOOP_MANAGER_SOCKET
(cd root && br init -q >/dev/null)
# Each rig is a repository of its own: a coder's commit stays in it, not in the repository the demo sits in. The one
# of a previous run goes: `git clean` leaves a repository in a directory with a tracked file.
for rig in frontend backend; do
  find "$rig/.git" -delete 2>/dev/null || true
  git -C "$rig" init -q
done
# The commit of the demo and of loop-manager, before the coders' work can change the tree.
sha=$(git describe --always --dirty 2>/dev/null || echo unknown)
# The agents call `loop-manager`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$loop_manager"):$PATH" "$loop_manager" daemon --config root/config.toml 2>.loop-manager/daemon.log &
daemon=$!
trap 'kill $daemon 2>/dev/null || true' EXIT
until "$loop_manager" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat .loop-manager/daemon.log; exit 1; }
  sleep 0.2
done

started=$(date +%s)
run=$("$loop_manager" run start planner-executor dev.deliver --input "task=$request")
result=passed
"$loop_manager" run wait "$run" --timeout 60m >.loop-manager/outcome.txt || result=failed
seconds=$(($(date +%s) - started))

versions="loop-manager-$("$loop_manager" --version | cut -d' ' -f2)@$sha"
for harness in claude codex; do
  versions="$versions,$harness-$("$harness" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
done
models=$(for pair in planner:root coder:frontend coder:backend; do
  agent=${pair%:*} rig=${pair#*:}
  "$loop_manager" config explain --config root/config.toml --project planner-executor --agent "dev.$agent" --rig "$rig" |
    awk -v who="$agent on $rig" '/^(model|harness) =/ {gsub(/"/, "", $3); v[$1] = $3}
      END {printf "%s: %s %s; ", who, v["harness"], v["model"]}'
done)
tokens=$(jq -rs '[.[] | select(.type == "run.passed" or .type == "run.failed") | .data.usage // empty]
  | "input \(map(.input_tokens) | add // 0), output \(map(.output_tokens) | add // 0)"' \
  "$XDG_STATE_HOME/loop-manager/journal/events.jsonl")
{
  echo "# $demo: $result"
  echo
  # Split so that the marker check does not take this template for a marker.
  echo "<!-- MEASURE" "id=demo-$demo date=$(date -u +%Y-%m-%d) ver=$versions env=$(uname -sm | tr ' ' '-')" \
    "src=demos/$demo/run.sh note=\"$result in $seconds s from the request, n=1\" -->"
  echo
  echo "Run on $(date -u +%Y-%m-%d) with $versions (\`$(uname -sm)\`): $result in $seconds s from the request."
  echo "Models: ${models%; }."
  echo "Tokens the harnesses reported for the runs: $tokens."
  echo
  echo "The request: \"$request\"."
  echo
  echo "The beads under the request (\`br dep tree <root> --direction up\`):"
  echo
  echo '```'
  (cd root && br dep tree "$("$loop_manager" --json run show "$run" | jq -r .bead)" --direction up --no-color)
  echo '```'
  echo
  echo "The runs, oldest first:"
  echo
  echo '| run | formula | step | rig | agent | state |'
  echo '|---|---|---|---|---|---|'
  "$loop_manager" --json run list --project planner-executor | jq -r 'sort_by(.created_at, (.id | length), .id) | .[] |
    "| \(.id) | \(.formula) | \(.step // "") | \(.workdir | split("/") | last) | \(.agent // "") | \(.state) |"'
  echo
  echo "The outcome of the run (\`loop-manager run wait\`):"
  echo
  echo '```'
  sed 's/[[:space:]]*$//' .loop-manager/outcome.txt
  echo '```'
} >RESULT.md
"$loop_manager" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

if [ "${EXAMPLE:-}" = 1 ]; then
  for rig in frontend backend; do
    find "$rig-example" -delete 2>/dev/null || true
    cp -R "$rig" "$rig-example"
    find "$rig-example" -name .gitkeep -delete
    find "$rig-example/.git" -delete
  done
  cp RESULT.md RESULT-example.md
fi
echo "$demo: $result in ${seconds}s; see RESULT.md, the daemon's log is .loop-manager/daemon.log"
[ "$result" = passed ]

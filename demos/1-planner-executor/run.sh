#!/bin/sh
# Runs the demo from the request to its report on a daemon of the demo's own. What the run creates stays in this
# directory and in .gitignore: the coders' work in frontend/ and backend/, the beads in root/.beads, the daemon's
# state in .aae/ and RESULT.md. EXAMPLE=1 also keeps the result as the author's frontend-example/, backend-example/
# and RESULT-example.md. AAE names the aae binary.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
demo=$(basename "$here")
aae=$(command -v "${AAE:-aae}") || { echo "no aae: install it or set AAE"; exit 1; }
request="Create me snake game in browser and a simple backend which store the high-scores, KISS"
cd "$here"
if [ -n "$(ls -A frontend backend root/.beads .aae 2>/dev/null | grep -v -e '^\.gitkeep$' -e ':$' -e '^$')" ]; then
  echo "$demo: a run is left over; start clean with: git clean -ffdX -- $here"
  exit 1
fi

mkdir -p .aae
export XDG_STATE_HOME="$here/.aae/state" XDG_CONFIG_HOME="$here/.aae/config" XDG_CACHE_HOME="$here/.aae/cache"
# A socket path has a length limit: it goes to a short temporary directory.
AAE_SOCKET="$(mktemp -d)/aae.sock"
export AAE_SOCKET
(cd root && br init -q >/dev/null)
# The agents call `aae`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$aae"):$PATH" "$aae" daemon --config root/config.toml 2>.aae/daemon.log &
daemon=$!
trap 'kill $daemon 2>/dev/null || true' EXIT
until "$aae" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat .aae/daemon.log; exit 1; }
  sleep 0.2
done

started=$(date +%s)
bead=$(jq -cn --arg request "$request" '{request: $request}')
run=$("$aae" run start planner-executor root dev.deliver --input "bead=$bead")
result=passed
"$aae" run wait "$run" --timeout 60m >.aae/outcome.txt || result=failed
seconds=$(($(date +%s) - started))

sha=$(git describe --always --dirty 2>/dev/null || echo unknown)
versions="aae-$("$aae" --version | cut -d' ' -f2)@$sha"
for harness in claude codex; do
  versions="$versions,$harness-$("$harness" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
done
models=$(for pair in planner:root coder:frontend coder:backend; do
  agent=${pair%:*} rig=${pair#*:}
  "$aae" config explain --config root/config.toml --project planner-executor --agent "dev.$agent" --rig "$rig" |
    awk -v who="$agent on $rig" '/^(model|harness) =/ {gsub(/"/, "", $3); v[$1] = $3}
      END {printf "%s: %s %s; ", who, v["harness"], v["model"]}'
done)
tokens=$(jq -rs '[.[] | select(.type == "run.passed" or .type == "run.failed") | .data.usage // empty]
  | "input \(map(.input_tokens) | add // 0), output \(map(.output_tokens) | add // 0)"' \
  "$XDG_STATE_HOME/aae/journal/events.jsonl")
{
  echo "# $demo: $result"
  echo
  # Split so that scripts/budget.sh does not take this template for a marker.
  echo "<!-- MEASURE" "id=demo-$demo date=$(date -u +%Y-%m-%d) ver=$versions env=$(uname -sm | tr ' ' '-')" \
    "src=demos/$demo/run.sh note=\"$result in $seconds s from the request, n=1\" -->"
  echo
  echo "Run on $(date -u +%Y-%m-%d) with $versions (\`$(uname -sm)\`): $result in $seconds s from the request."
  echo "Models: ${models%; }."
  echo "Tokens the harnesses reported for the runs: $tokens."
  echo
  echo "The request: \"$request\"."
  echo
  echo "The beads (\`br list --all --tree\`):"
  echo
  echo '```'
  (cd root && br list --all --tree --no-color)
  echo '```'
  echo
  echo "The runs, oldest first:"
  echo
  echo '| run | formula | step | rig | agent | state |'
  echo '|---|---|---|---|---|---|'
  "$aae" --json run list --project planner-executor | jq -r 'sort_by(.created_at, (.id | length), .id) | .[] |
    "| \(.id) | \(.formula) | \(.step // "") | \(.workdir | split("/") | last) | \(.agent // "") | \(.state) |"'
  echo
  echo "The outcome of the run (\`aae run wait\`):"
  echo
  echo '```'
  sed 's/[[:space:]]*$//' .aae/outcome.txt
  echo '```'
} >RESULT.md
"$aae" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

if [ "${EXAMPLE:-}" = 1 ]; then
  for rig in frontend backend; do
    find "$rig-example" -delete 2>/dev/null || true
    cp -R "$rig" "$rig-example"
    find "$rig-example" -name .gitkeep -delete
  done
  cp RESULT.md RESULT-example.md
fi
echo "$demo: $result in ${seconds}s; see RESULT.md, the daemon's log is .aae/daemon.log"
[ "$result" = passed ]

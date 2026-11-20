#!/bin/sh
# Runs the demo from the request to its report on a daemon of the demo's own, with the bridge that gives the review's
# outcome from GitHub (bridge.sh). The three repositories of the project are clones of the GitHub repositories of
# OWNER (kvokka-agent by default), in gate-demo/, gate-demo-backend/ and gate-demo-frontend/; the beads are in
# root/.beads, the review the reviser gets in root/feedback, the daemon's state and the bridge's log in .loop-manager/ and the
# report in RESULT.md, all in .gitignore. EXAMPLE=1 also keeps the result as the author's *-example. LOOP_MANAGER names the loop-manager
# binary. `gh` must be logged in as an account that may push to the repositories, or GITHUB_TOKEN hold its token.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
demo=$(basename "$here")
loop_manager=$(command -v "${LOOP_MANAGER:-loop-manager}") || { echo "no loop-manager: install it or set LOOP_MANAGER"; exit 1; }
owner=${OWNER:-kvokka-agent}
repos="gate-demo gate-demo-backend gate-demo-frontend"
request="Let a visitor of the site type their name and see a greeting the backend makes for it, KISS"
cd "$here"
# shellcheck disable=SC2086 # $repos is a list
if find $repos root/.beads root/feedback .loop-manager -mindepth 1 -maxdepth 1 ! -name .gitkeep ! -name .git 2>/dev/null |
  grep -q .; then
  echo "$demo: a run is left over; start clean with: git clean -ffdX -- $here"
  exit 1
fi

# A step gets a clean environment and an agent the environment of its config, which a login of gh in a keyring does
# not reach: the steps (`[exec]` of root/config.toml) and the agent `owner` take the daemon's GITHUB_TOKEN instead.
GITHUB_TOKEN=${GITHUB_TOKEN:-$(gh auth token)} || { echo "$demo: log gh in or set GITHUB_TOKEN"; exit 1; }
export GITHUB_TOKEN

# Each repository is a clone of its GitHub repository; git pushes with the login of gh. The one of a previous run
# goes: `git clean` leaves a repository in a directory with a tracked file.
for repo in $repos; do
  find "$repo/.git" -delete 2>/dev/null || true
  git -C "$repo" init -q
  git -C "$repo" config credential.helper '!gh auth git-credential'
  git -C "$repo" remote add origin "https://github.com/$owner/$repo.git"
  git -C "$repo" fetch -q origin
  git -C "$repo" remote set-head origin -a >/dev/null
  base=$(git -C "$repo" rev-parse --abbrev-ref origin/HEAD)
  git -C "$repo" checkout -q -B "${base#origin/}" "$base"
  echo .gitkeep >>"$repo/.git/info/exclude"
done

mkdir -p .loop-manager
export XDG_STATE_HOME="$here/.loop-manager/state" XDG_CONFIG_HOME="$here/.loop-manager/config" XDG_CACHE_HOME="$here/.loop-manager/cache"
# A socket path has a length limit: it goes to a short temporary directory.
LOOP_MANAGER_SOCKET="$(mktemp -d)/loop-manager.sock"
export LOOP_MANAGER_SOCKET
(cd root && br init -q >/dev/null)
# The commit of the demo and of loop-manager, before the run can change the tree.
sha=$(git describe --always --dirty 2>/dev/null || echo unknown)
# What run.sh starts in the background is stopped when it ends, on a signal too: dash runs no EXIT trap for one.
daemon='' bridge=''
trap 'kill $bridge $daemon 2>/dev/null || true' EXIT
trap 'exit 1' INT TERM
# The agents and the steps call `loop-manager`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$loop_manager"):$PATH" "$loop_manager" daemon --config root/config.toml 2>.loop-manager/daemon.log &
daemon=$!
until "$loop_manager" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat .loop-manager/daemon.log; exit 1; }
  sleep 0.2
done
# The bridge gives the review's outcome from GitHub to the feature that waits on it.
LOOP_MANAGER="$loop_manager" ./bridge.sh review-gate 2>.loop-manager/bridge.log &
bridge=$!

started=$(date +%s)
run=$("$loop_manager" run start review-gate dev.feature --input "task=$request")
result=passed
"$loop_manager" run wait "$run" --timeout 120m >.loop-manager/outcome.txt || result=failed
kill $bridge
seconds=$(($(date +%s) - started))
show=$("$loop_manager" --json run show "$run")
rounds=$(printf '%s' "$show" | jq -r .round)
pr=$(printf '%s' "$show" | jq -r '.steps.pr.data.stdout // "" | rtrimstr("\n") | split("\n") | last // ""')

versions="loop-manager-$("$loop_manager" --version | cut -d' ' -f2)@$sha"
for tool in codex gh; do
  versions="$versions,$tool-$("$tool" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
done
models=$(for pair in dev.planner:root dev.coder:backend dev.coder:frontend dev.reviser:root review-gate.owner:root; do
  agent=${pair%:*} rig=${pair#*:}
  "$loop_manager" config explain --config root/config.toml --project review-gate --agent "$agent" --rig "$rig" |
    awk -v who="${agent#*.} on $rig" '/^(model|harness) =/ {gsub(/"/, "", $3); v[$1] = $3}
      END {printf "%s: %s %s; ", who, v["harness"], v["model"]}'
done)
{
  echo "# $demo: $result"
  echo
  # Split so that the marker check does not take this template for a marker.
  echo "<!-- MEASURE" "id=demo-$demo date=$(date -u +%Y-%m-%d) ver=$versions env=$(uname -sm | tr ' ' '-')" \
    "src=demos/$demo/run.sh note=\"$result in $seconds s and $rounds rounds from the request, n=1\" -->"
  echo
  echo "Run on $(date -u +%Y-%m-%d) with $versions (\`$(uname -sm)\`): $result in $seconds s from the request,"
  echo "after $rounds round(s) of the review."
  echo "Models: ${models%; }."
  echo
  echo "The request: \"$request\". The root's pull request: ${pr:-none}."
  echo
  echo "The runs, oldest first:"
  echo
  echo '| run | formula | step | rig | agent | state |'
  echo '|---|---|---|---|---|---|'
  "$loop_manager" --json run list --project review-gate | jq -r 'sort_by(.created_at, (.id | length), .id) | .[] |
    "| \(.id) | \(.formula) | \(.step // "") | \(.workdir | split("/") | last) | \(.agent // "") | \(.state) |"'
  echo
  echo "The outcome of the run (\`loop-manager run wait\`):"
  echo
  echo '```'
  # The paths under the demo relative to it: a result names no directory of the machine it ran on.
  sed -e 's/[[:space:]]*$//' -e "s|$here/||g" .loop-manager/outcome.txt
  echo '```'
} >RESULT.md
"$loop_manager" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

if [ "${EXAMPLE:-}" = 1 ]; then
  for repo in $repos; do
    find "$repo-example" -delete 2>/dev/null || true
    cp -R "$repo" "$repo-example"
    find "$repo-example" -name .gitkeep -delete
    find "$repo-example/.git" -delete
  done
  cp RESULT.md RESULT-example.md
fi
echo "$demo: $result in ${seconds}s after $rounds round(s); see RESULT.md, the daemon's log is .loop-manager/daemon.log"
[ "$result" = passed ]

#!/bin/sh
# Runs the demo from its two requests to its report on a daemon of the demo's own. What the run creates stays in this
# directory and in .gitignore: the documents and what the agents wrote in library/, the beads in root/.beads, the
# daemon's state in .loop-manager/ and RESULT.md. EXAMPLE=1 also keeps the result as the author's library-example/ and
# RESULT-example.md. LOOP_MANAGER names the loop-manager binary.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
demo=$(basename "$here")
loop_manager=$(command -v "${LOOP_MANAGER:-loop-manager}") || { echo "no loop-manager: install it or set LOOP_MANAGER"; exit 1; }
glossary="List every term the documents in docs/ define, each with a one-line definition and the document that defines it, in GLOSSARY.md"
index="Write INDEX.md: for each document in docs/, its sections, each with one line on what it says"
cd "$here"
if [ -n "$(ls -A library root/.beads .loop-manager 2>/dev/null | grep -v -e '^\.gitkeep$' -e ':$' -e '^$')" ]; then
  echo "$demo: a run is left over; start clean with: git clean -ffdX -- $here"
  exit 1
fi

mkdir -p .loop-manager
export XDG_STATE_HOME="$here/.loop-manager/state" XDG_CONFIG_HOME="$here/.loop-manager/config" XDG_CACHE_HOME="$here/.loop-manager/cache"
# A socket path has a length limit: it goes to a short temporary directory.
LOOP_MANAGER_SOCKET="$(mktemp -d)/loop-manager.sock"
export LOOP_MANAGER_SOCKET
(cd root && br init -q >/dev/null)
# What the agents read: the documentation of loop-manager as this checkout has it, about 100000 tokens.
cp -R ../../docs library/docs
sha=$(git describe --always --dirty 2>/dev/null || echo unknown)
# The agents and the addon's hook call `loop-manager`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$loop_manager"):$PATH" "$loop_manager" daemon --config root/config.toml 2>.loop-manager/daemon.log &
daemon=$!
trap 'kill $daemon 2>/dev/null || true' EXIT
until "$loop_manager" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat .loop-manager/daemon.log; exit 1; }
  sleep 0.2
done

# The two runs side by side, each with its own request and so its own bead.
started=$(date +%s)
"$loop_manager" run start library study.glossary --input "task=$glossary" >.loop-manager/glossarist.run
"$loop_manager" run start library study.index --input "task=$index" >.loop-manager/indexer.run
result=passed
for agent in glossarist indexer; do
  "$loop_manager" run wait "$(cat .loop-manager/$agent.run)" --timeout 60m >".loop-manager/$agent.txt" || result=failed
done
seconds=$(($(date +%s) - started))
for agent in glossarist indexer; do
  "$loop_manager" --json run show "$(cat .loop-manager/$agent.run)" | jq -r .bead >".loop-manager/$agent.bead"
done
# The demo shows the wrap-up of the glossarist at the addon's soft limit: a bead without its summary comment fails it.
(cd root && br comments list --json "$(cat ../.loop-manager/glossarist.bead)") >.loop-manager/glossarist.comments
jq -e 'length > 0' .loop-manager/glossarist.comments >/dev/null || result=failed

versions="loop-manager-$("$loop_manager" --version | cut -d' ' -f2)@$sha"
for harness in claude codex; do
  versions="$versions,$harness-$("$harness" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
done
{
  echo "# $demo: $result"
  echo
  # Split so that scripts/budget.sh does not take this template for a marker.
  echo "<!-- MEASURE" "id=demo-$demo date=$(date -u +%Y-%m-%d) ver=$versions env=$(uname -sm | tr ' ' '-')" \
    "src=demos/$demo/run.sh note=\"$result in $seconds s from the requests, n=1\" -->"
  echo
  echo "Run on $(date -u +%Y-%m-%d) with $versions (\`$(uname -sm)\`): $result in $seconds s from the requests."
  for agent in glossarist indexer; do
    echo
    echo "## $agent"
    echo
    "$loop_manager" config explain --config root/config.toml --project library --agent "study.$agent" |
      grep -e '^harness =' -e '^model =' -e '^native\.' -e '^addons\.context-budget\.params\.' | sed 's/^/    /'
    echo
    echo "The request: \"$(if [ $agent = glossarist ]; then echo "$glossary"; else echo "$index"; fi)\"."
    echo
    if [ $agent = glossarist ]; then
      echo "The comments on its bead \`$(cat .loop-manager/$agent.bead)\` (\`br comments list\`):"
      echo
      echo '```'
      jq -r '.[] | "[\(.author)] at \(.created_at)\n\(.text)\n"' .loop-manager/glossarist.comments
      echo '```'
      echo
    fi
    echo "The outcome of the run (\`loop-manager run wait\`):"
    echo
    echo '```'
    sed 's/[[:space:]]*$//' ".loop-manager/$agent.txt"
    echo '```'
  done
  echo
  echo "The files the addon kept, per session (\`<state>/loop-manager/addons/context-budget/\`):"
  echo
  echo '```'
  ls "$XDG_STATE_HOME/loop-manager/addons/context-budget" 2>/dev/null || true
  echo '```'
} >RESULT.md
"$loop_manager" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

if [ "${EXAMPLE:-}" = 1 ]; then
  find library-example -delete 2>/dev/null || true
  mkdir library-example
  cp library/GLOSSARY.md library/INDEX.md library-example/ 2>/dev/null || true
  cp RESULT.md RESULT-example.md
fi
echo "$demo: $result in ${seconds}s; see RESULT.md, the daemon's log is .loop-manager/daemon.log"
[ "$result" = passed ]

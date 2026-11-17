#!/bin/sh
# Runs the demo from its two requests to its report on a daemon of the demo's own. What the run creates stays in this
# directory and in .gitignore: the documents and what the agents wrote in library/, the beads in root/.beads, the
# daemon's state in .aae/ and RESULT.md. EXAMPLE=1 also keeps the result as the author's library-example/ and
# RESULT-example.md. AAE names the aae binary.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
demo=$(basename "$here")
aae=$(command -v "${AAE:-aae}") || { echo "no aae: install it or set AAE"; exit 1; }
glossary="List every term the documents in docs/ define, each with a one-line definition and the document that defines it, in GLOSSARY.md"
index="Write INDEX.md: for each document in docs/, its sections, each with one line on what it says"
cd "$here"
if [ -n "$(ls -A library root/.beads .aae 2>/dev/null | grep -v -e '^\.gitkeep$' -e ':$' -e '^$')" ]; then
  echo "$demo: a run is left over; start clean with: git clean -ffdX -- $here"
  exit 1
fi

mkdir -p .aae
export XDG_STATE_HOME="$here/.aae/state" XDG_CONFIG_HOME="$here/.aae/config" XDG_CACHE_HOME="$here/.aae/cache"
# A socket path has a length limit: it goes to a short temporary directory.
AAE_SOCKET="$(mktemp -d)/aae.sock"
export AAE_SOCKET
(cd root && br init -q >/dev/null)
# What the agents read: the documentation of aae as this checkout has it, about 100000 tokens.
cp -R ../../docs library/docs
sha=$(git describe --always --dirty 2>/dev/null || echo unknown)
# The agents and the addon's hook call `aae`: the binary goes first on the PATH the daemon hands them.
PATH="$(dirname "$aae"):$PATH" "$aae" daemon --config root/config.toml 2>.aae/daemon.log &
daemon=$!
trap 'kill $daemon 2>/dev/null || true' EXIT
until "$aae" status >/dev/null 2>&1; do
  kill -0 $daemon 2>/dev/null || { cat .aae/daemon.log; exit 1; }
  sleep 0.2
done

# The two runs side by side, each with its own request and so its own bead.
started=$(date +%s)
"$aae" run start library study.glossary --input "task=$glossary" >.aae/glossarist.run
"$aae" run start library study.index --input "task=$index" >.aae/indexer.run
result=passed
for agent in glossarist indexer; do
  "$aae" run wait "$(cat .aae/$agent.run)" --timeout 60m >".aae/$agent.txt" || result=failed
done
seconds=$(($(date +%s) - started))
# The demo shows a compaction of each agent: a bead without the addon's label fails it.
for agent in glossarist indexer; do
  "$aae" --json run show "$(cat .aae/$agent.run)" | jq -r .bead >".aae/$agent.bead"
  (cd root && br label list "$(cat ../.aae/$agent.bead)") >".aae/$agent.labels"
  grep -q 'compactions:' ".aae/$agent.labels" || result=failed
done

versions="aae-$("$aae" --version | cut -d' ' -f2)@$sha"
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
    "$aae" config explain --config root/config.toml --project library --agent "study.$agent" |
      grep -e '^harness =' -e '^model =' -e '^native\.' -e '^addons\.context-budget\.params\.' | sed 's/^/    /'
    echo
    echo "The request: \"$(if [ $agent = glossarist ]; then echo "$glossary"; else echo "$index"; fi)\"."
    echo
    echo "The labels of its bead \`$(cat .aae/$agent.bead)\` (\`br label list\`):"
    echo
    echo '```'
    cat ".aae/$agent.labels"
    echo '```'
    echo
    echo "The outcome of the run (\`aae run wait\`):"
    echo
    echo '```'
    sed 's/[[:space:]]*$//' ".aae/$agent.txt"
    echo '```'
  done
  echo
  echo "The compactions the addon counted, per session (\`<state>/aae/addons/context-budget/<session>.count\`):"
  echo
  echo '```'
  for count in "$XDG_STATE_HOME"/aae/addons/context-budget/*.count; do
    if [ -e "$count" ]; then echo "$(basename "$count" .count): $(cat "$count")"; fi
  done
  echo '```'
} >RESULT.md
"$aae" drain >/dev/null 2>&1 || true
wait "$daemon" 2>/dev/null || true

if [ "${EXAMPLE:-}" = 1 ]; then
  find library-example -delete 2>/dev/null || true
  mkdir library-example
  cp library/GLOSSARY.md library/INDEX.md library-example/ 2>/dev/null || true
  cp RESULT.md RESULT-example.md
fi
echo "$demo: $result in ${seconds}s; see RESULT.md, the daemon's log is .aae/daemon.log"
[ "$result" = passed ]

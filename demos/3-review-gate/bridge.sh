#!/bin/sh
# The bridge between GitHub and the runs of a project that wait on their review (README, "The bridge"). It follows the
# daemon's events and, while a run waits, looks every INTERVAL seconds (30 by default) at the pull requests of its
# feature, the root's from the last line of the step `pr` and the others by the branch `feature/<bead>`:
# - the root's merged: `pass`, with who merged it in the data;
# - the root's closed without a merge: the run is cancelled;
# - a comment or a review of an author in the run's `feedback_from` that the account of `gh` has not marked with the
#   reaction "eyes" (the step `collect` marks each it takes): `fail`, with the comment's address in the summary.
# Anyone else's comments change nothing. Usage: bridge.sh <project>; LOOP_MANAGER names the loop-manager binary.
set -eu
project=$1
loop_manager=${LOOP_MANAGER:-loop-manager}
interval=${INTERVAL:-30}
fields='id url body author { login } reactionGroups { content viewerHasReacted }'
query="query(\$url: URI!) { resource(url: \$url) { ... on PullRequest {
  comments(first: 100) { nodes { $fields } }
  reviews(first: 100) { nodes { $fields comments(first: 100) { nodes { $fields } } } } } } }"

# Gives the outcome of `review` to the run $1 waiting on it, or cancels the run, once GitHub has one.
look() {
  show=$("$loop_manager" --json run show "$1")
  root=$(printf '%s' "$show" | jq -r '.steps.pr.data.stdout // "" | rtrimstr("\n") | split("\n") | last // ""')
  [ -n "$root" ] || return 0
  # One key per round: a repeat of the same outcome changes nothing, the next round's is new.
  key="bridge-$1-$(printf '%s' "$show" | jq -r .round)"
  pr=$(gh pr view "$root" --json state,mergedBy) || return 1
  case $(printf '%s' "$pr" | jq -r .state) in
    MERGED)
      "$loop_manager" run resolve "$1" review pass --summary "merged: $root" --key "$key" \
        --data "$(printf '%s' "$pr" | jq -c '{merged_by: .mergedBy.login}')" >/dev/null
      return ;;
    CLOSED)
      "$loop_manager" run cancel "$1" --reason "closed without a merge: $root" --key "$key" >/dev/null
      return ;;
  esac
  who=$(printf '%s' "$show" | jq -r '.inputs.feedback_from // ""')
  branch="feature/$(printf '%s' "$show" | jq -r .bead)"
  places=$("$loop_manager" --json project list --project "$project")
  for repo in $(printf '%s' "$places" | jq -r '.[0] | .root.path, .rigs[].path'); do
    url=$(cd "$repo" && gh pr view "$branch" --json url -q .url 2>/dev/null) || continue
    reply=$(gh api graphql -f query="$query" -f url="$url") || return 1
    new=$(printf '%s' "$reply" | jq -r --arg who "$who" '
      ($who | split(",") | map(gsub("^\\s+|\\s+$"; ""))) as $who
      | .data.resource | .comments.nodes + [.reviews.nodes[] | ., .comments.nodes[]] | .[]
      | select(.body != "" and (.author.login as $a | $who | index($a)))
      | select(any(.reactionGroups[]; .content == "EYES" and .viewerHasReacted) | not) | .url' | head -1)
    if [ -n "$new" ]; then
      "$loop_manager" run resolve "$1" review fail --summary "review: $new" --key "$key" >/dev/null
      return
    fi
  done
}

# The events from now on: a run that starts to wait wakes the bridge at once.
events=$(mktemp)
"$loop_manager" --json events >"$events" &
follower=$!
trap 'kill $follower 2>/dev/null || true' EXIT
trap 'exit 0' INT TERM
seen=0
while :; do
  runs=$("$loop_manager" --json run list --awaiting --project "$project" | jq -r '.[] | select(.step == "review") | .id')
  for run in $runs; do
    look "$run" || echo "bridge: $run: no answer from GitHub or loop-manager, asking again in ${interval}s" >&2
  done
  # GitHub is asked only while a run waits; with none, a new `run.awaiting` ends the wait, and so does the interval,
  # which covers one that came before the events were followed.
  waited=0
  while [ "$waited" -lt "$interval" ]; do
    if [ -z "$runs" ] && tail -n "+$((seen + 1))" "$events" | grep -q '"run.awaiting"'; then break; fi
    sleep 1
    waited=$((waited + 1))
  done
  seen=$(wc -l <"$events")
done

#!/bin/sh
# A fake `gh` for the test of demo 3 (review_gate.rs): the pull requests live in $GH_FAKE (`github` next to this
# script when unset), one directory per number, with the files `repo` (the top of the git repository it was opened
# from), `branch`, `state`, `url`, `merged_by` and `comments`, a JSON object per line. The account is $GH_FAKE_USER (`agent` when unset). Only the calls of the demo's
# scripts and bridge are here; anything else fails.
set -eu
GH_FAKE=${GH_FAKE:-$(dirname "$0")/github}
mkdir -p "$GH_FAKE"
me=${GH_FAKE_USER:-agent}
fail() { echo "fake gh: $*" >&2; exit 1; }

# The directory of the pull request $1, a URL or a branch of the repository of the working directory.
find_pr() {
  found=
  for pr in "$GH_FAKE"/*/; do
    [ -d "$pr" ] || continue
    pr=${pr%/}
    case $1 in
      https://*) [ "$(cat "$pr/url")" = "$1" ] && found=$pr ;;
      *) [ "$(cat "$pr/branch")" = "$1" ] && [ "$(cat "$pr/repo")" = "$(git rev-parse --show-toplevel)" ] && found=$pr ;;
    esac
  done
  [ -n "$found" ] || fail "no pull request found for $1"
  echo "$found"
}

# The value of the option $1 among the rest of the arguments.
opt() {
  name=$1
  shift
  while [ $# -gt 1 ]; do
    [ "$1" = "$name" ] && { echo "$2"; return; }
    shift
  done
}

# The value of `-f $1=<value>` among the rest of the arguments.
field() {
  name=$1
  shift
  while [ $# -gt 1 ]; do
    if [ "$1" = -f ]; then
      case $2 in "$name="*) echo "${2#"$name"=}"; return ;; esac
    fi
    shift
  done
}

case "$1 ${2:-}" in
  "pr view")
    pr=$(find_pr "$3")
    view=$(jq -n --arg state "$(cat "$pr/state")" --arg url "$(cat "$pr/url")" --arg by "$(cat "$pr/merged_by")" \
      '{state: $state, url: $url, mergedBy: (if $by == "" then null else {login: $by} end)}')
    filter=$(opt -q "$@")
    printf '%s\n' "$view" | jq -r "${filter:-.}" ;;
  "pr create")
    n=$(($(find "$GH_FAKE" -mindepth 1 -maxdepth 1 -type d | wc -l) + 1))
    pr="$GH_FAKE/$n"
    mkdir -p "$pr"
    top=$(git rev-parse --show-toplevel)
    echo "$top" >"$pr/repo"
    opt -H "$@" >"$pr/branch"
    echo OPEN >"$pr/state"
    echo "https://github.test/$(basename "$top")/pull/$n" >"$pr/url"
    : >"$pr/merged_by"
    : >"$pr/comments"
    cat "$pr/url" ;;
  "pr edit") find_pr "$3" >/dev/null ;;
  "pr merge")
    pr=$(find_pr "$3")
    [ "$(cat "$pr/state")" = OPEN ] || fail "$3 is not open"
    echo MERGED >"$pr/state"
    echo "$me" >"$pr/merged_by" ;;
  "pr close")
    pr=$(find_pr "$3")
    echo CLOSED >"$pr/state" ;;
  "pr comment")
    pr=$(find_pr "$3")
    n=$(($(cat "$GH_FAKE"/*/comments | wc -l) + 1))
    jq -nc --arg id "C$n" --arg url "$(cat "$pr/url")#issuecomment-$n" --arg body "$(opt --body "$@")" --arg who "$me" \
      '{id: $id, url: $url, body: $body, author: {login: $who}, eyes: false}' >>"$pr/comments" ;;
  "api graphql")
    id=$(field id "$@")
    if [ -n "$id" ]; then
      for comments in "$GH_FAKE"/*/comments; do
        jq -c --arg id "$id" 'if .id == $id then .eyes = true else . end' "$comments" >"$comments.new"
        mv "$comments.new" "$comments"
      done
      echo '{"data": {"addReaction": {"clientMutationId": null}}}'
    else
      pr=$(find_pr "$(field url "$@")")
      jq -s '{data: {resource: {reviews: {nodes: []}, comments: {nodes: map(
        del(.eyes) + {reactionGroups: (if .eyes then [{content: "EYES", viewerHasReacted: true}] else [] end)})}}}}' \
        "$pr/comments"
    fi ;;
  *) fail "not faked: $*" ;;
esac

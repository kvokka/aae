#!/bin/sh
# Record an external await answer and return the work to a fresh coder. Exit 1 is the check's intentional return;
# exit 75 is an operational failure, retried as an attempt instead of spending a round without an answer.
set -u
case ${1:-} in
  code)
    request=${LOOP_MANAGER_STEP_CODE_DATA:-null}
    summary=${LOOP_MANAGER_STEP_CODE_OWNER_SUMMARY:-}
    data=${LOOP_MANAGER_STEP_CODE_OWNER_DATA:-null}
    ;;
  review)
    request=${LOOP_MANAGER_STEP_REVIEW_DATA:-null}
    summary=${LOOP_MANAGER_STEP_REVIEW_OWNER_SUMMARY:-}
    data=${LOOP_MANAGER_STEP_REVIEW_OWNER_DATA:-null}
    ;;
  *) echo 'owner-answer: expected code or review' >&2; exit 75 ;;
esac
bead=${LOOP_MANAGER_BEAD:?}
if ! printf '%s' "$data" | jq -e --arg summary "$summary" '
  ($summary | test("\\S")) or (. != null and . != {} and . != [] and
    (if type == "string" then test("\\S") else true end))' >/dev/null; then
  echo 'owner-answer: resolve with a non-empty summary or answer data' >&2
  exit 75
fi
id=$(printf '%s' "$request" | jq -er '.question_id | strings | select(test("^[A-Za-z0-9][A-Za-z0-9._:-]*$"))') || exit 75
dir=target/devloop/runs/${LOOP_MANAGER_RUN:?}
mkdir -p "$dir" || exit 75
answer=$(mktemp "$dir/answer.XXXXXX") || exit 75
{
  printf 'ANSWER: %s\n%s\n' "$id" "$summary"
  if [ "$data" != null ]; then printf '%s' "$data" | loop-manager format || exit 75; fi
} >"$answer" || exit 75
# A process may be retried after its comment reached the tracker; do not append that answer twice.
existing=$(br comments "$bead" --json) || exit 75
recorded=$(printf '%s' "$existing" | jq -r --arg id "$id" --rawfile answer "$answer" '
  [.[] | select((.text | split("\n")[0]) as $header |
    ["QUESTION: " + $id, "DECISION: " + $id, "ANSWER: " + $id] | index($header))]
  | last | .text == $answer or .text == ($answer | rtrimstr("\n"))') || exit 75
if [ "$recorded" != true ]; then br comments add "$bead" -f "$answer" -q || exit 75; fi
# Other concerns may remain unresolved; the label is reconciled from all the comments, not removed here.
cat "$answer" || exit 75
exit 1

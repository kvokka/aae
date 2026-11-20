#!/bin/sh
# The daemon keeps the last return's remarks across restarts. A review or an owner answer starts a fresh streak.
set -eu
report=$(printf '%s\n' "${LOOP_MANAGER_STEP_CI_REPORT_DATA:?}" | jq -er '.stdout | fromjson | select(.exit_code | type == "number")')
if [ "$(printf '%s\n' "$report" | jq .exit_code)" = 0 ]; then exit 0; fi

case ${LOOP_MANAGER_REMARKS:-} in
  'ci_route: exit 1
CI return 1/3'*) n=2;;
  'ci_route: exit 1
CI return 2/3'*) n=3;;
  'ci_route: exit 1
CI return 3/3'*) echo "CI is still red after three coder returns; sending the report to review."; exit 0;;
  *) n=1;;
esac
echo "CI return $n/3"
printf '%s\n' "$report" | jq -r '"CI exit \(.exit_code) at \(.head)", (.failed[] | "\(.name): \(.status) (exit \(.exit_code))"), "LOG: \(.log)"'
exit 1

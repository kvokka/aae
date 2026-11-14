#!/usr/bin/env bash
# Enforces the line budgets and measurement rules of spec §14.
# Usage: scripts/budget.sh [repo-root]   (defaults to the current git repository)
set -euo pipefail
cd "${1:-$(git rev-parse --show-toplevel)}"

fail=0
err() { echo "budget: $*" >&2; fail=1; }

# Product code is everything under crates/*/src except test-only crates, test files and generated code.
mapfile -t files < <(git ls-files -co --exclude-standard -- 'crates/*/src/*' \
  | grep -Ev '^crates/aae-(testkit|acceptance)/|(^|/)tests(\.rs$|/)|/generated/' || true)

# Code lines (no blanks, no comments) per budget part, counted by tokei.
declare -A limit=([core]=12000 [claude]=3500 [codex]=3500 [tracker]=1200 [frontends]=4000 [plugins]=1500 [reserve]=4300)
declare -A used=()
total=0
if ((${#files[@]})); then
  while IFS=$'\t' read -r code path; do
    case "$path" in
      crates/aae-core/*) part=core ;;
      crates/aae-claude/*) part=claude ;;
      crates/aae-codex/*) part=codex ;;
      crates/aae-br/*) part=tracker ;;
      crates/aae-front/*) part=frontends ;;
      crates/aae-plugins/*) part=plugins ;;
      crates/aae/*) part=reserve ;;
      *) err "$path belongs to no budget part (spec §14)"; continue ;;
    esac
    used[$part]=$((${used[$part]:-0} + code))
    total=$((total + code))
  done < <(tokei -o json "${files[@]}" \
    | jq -r 'to_entries[] | select(.key != "Total") | .value.reports[] | "\(.stats.code)\t\(.name)"')

  for f in "${files[@]}"; do
    n=$(wc -l <"$f")
    ((n <= 400)) || err "$f has $n lines, the limit is 400"
  done
fi

for part in core claude codex tracker frontends plugins reserve; do
  printf 'budget: %-9s %6d / %d\n' "$part" "${used[$part]:-0}" "${limit[$part]}"
  ((${used[$part]:-0} <= limit[$part])) || err "$part exceeds its limit"
done
printf 'budget: %-9s %6d / 30000 (hard stop 35000)\n' total "$total"
((total <= 30000)) || err "release budget exceeded"
((total <= 35000)) || err "HARD STOP exceeded: no new features until a deletion pass"

# Spec §16: the throwaway experiments stay within 5 000 code lines in total (generated code excluded, as above).
mapfile -t exp < <(git ls-files -co --exclude-standard -- 'experiments/*.rs' | grep -v '/generated/' || true)
if ((${#exp[@]})); then
  n=$(tokei -o json "${exp[@]}" | jq '.Total.code')
  printf 'budget: %-9s %6d / 5000\n' experiments "$n"
  ((n <= 5000)) || err "experiments exceed 5000 lines (spec §16)"
fi

# Every MEASURE marker carries non-empty id, date, ver and env. `id=<...>` is the format template, not a marker.
rc=0
markers=$(git -c submodule.recurse=false grep -nE --untracked '(<!--|//|#) MEASURE id=' -- . \
  ':!.loop' ':!.claude' ':!scripts/budget.sh' ':!scripts/budget-selftest.sh') || rc=$?
((rc <= 1)) || err "git grep failed with code $rc"
while IFS= read -r line; do
  [[ -z "$line" || "$line" =~ MEASURE\ id=\< ]] && continue
  bare=$(sed 's/"[^"]*"//g' <<<"$line") # keys inside quoted values (note="... ver=1") do not count
  for key in id ver env; do
    [[ "$bare" =~ [[:space:]]$key=[^[:space:]\"] ]] || err "MEASURE marker without $key: $line"
  done
  [[ "$bare" =~ [[:space:]]date=[0-9]{4}-[0-9]{2}-[0-9]{2} ]] || err "MEASURE marker without date: $line"
done <<<"$markers"

[[ -f docs/READINESS.md ]] || err "docs/READINESS.md is missing (spec §14)"
grep -q 'MEASURE id=' docs/READINESS.md 2>/dev/null || err "docs/READINESS.md has no MEASURE marker"
# Criterion 22 of spec §15: every component of the readiness table scores at least 8.
rows=$(awk -F'|' '/^\| / && $3 !~ /Score/ {print $2 "|" $3}' docs/READINESS.md 2>/dev/null || true)
if [[ -z "$rows" ]]; then
  err "docs/READINESS.md has no component table"
else
  while IFS='|' read -r name score; do
    [[ "$score" =~ ^\ *([0-9]+(\.[0-9]+)?)\ *$ ]] && awk "BEGIN { exit !(${BASH_REMATCH[1]} >= 8) }" ||
      err "readiness of${name% } is${score% }, criterion 22 needs at least 8"
  done <<<"$rows"
fi

exit "$fail"

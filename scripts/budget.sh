#!/usr/bin/env bash
# Enforces the line budgets and measurement rules of docs/agents/rules-code.md.
# Usage: scripts/budget.sh [repo-root]   (defaults to the current git repository)
set -euo pipefail
cd "${1:-$(git rev-parse --show-toplevel)}"

fail=0
err() { echo "budget: $*" >&2; fail=1; }

# Product code is everything under crates/*/src except test-only crates, test files and generated code.
mapfile -t files < <(git ls-files -co --exclude-standard -- 'crates/*/src/*' \
  | grep -Ev '^crates/loop-manager-(testkit|acceptance)/|(^|/)tests(\.rs$|/)|/generated/' || true)

# Code lines (no blanks, no comments) per budget part, counted by tokei.
declare -A limit=([core]=18000 [claude]=3500 [codex]=3500 [tracker]=1200 [frontends]=4000 [addons]=1500 [reserve]=4300)
declare -A used=()
total=0
if ((${#files[@]})); then
  while IFS=$'\t' read -r code path; do
    case "$path" in
      crates/loop-manager-core/* | crates/loop-manager-contract/*) part=core ;;
      crates/loop-manager-claude/*) part=claude ;;
      crates/loop-manager-codex/*) part=codex ;;
      crates/loop-manager-br/*) part=tracker ;;
      crates/loop-manager-front/*) part=frontends ;;
      crates/loop-manager-addons/*) part=addons ;;
      crates/loop-manager/*) part=reserve ;;
      *) err "$path belongs to no budget part (docs/agents/rules-code.md)"; continue ;;
    esac
    used[$part]=$((${used[$part]:-0} + code))
    total=$((total + code))
  done < <(tokei -o json "${files[@]}" \
    | jq -r 'to_entries[] | select(.key != "Total") | .value.reports[] | "\(.stats.code)\t\(.name)"')

fi

# The file limit holds for every Rust file, tests and test-only crates included.
while IFS= read -r f; do
  n=$(wc -l <"$f")
  ((n <= 400)) || err "$f has $n lines, the limit is 400"
done < <(git ls-files -co --exclude-standard -- 'crates/*.rs' | grep -v '/generated/' || true)

for part in core claude codex tracker frontends addons reserve; do
  printf 'budget: %-9s %6d / %d\n' "$part" "${used[$part]:-0}" "${limit[$part]}"
  ((${used[$part]:-0} <= limit[$part])) || err "$part exceeds its limit"
done
printf 'budget: %-9s %6d / 30000 (hard stop 35000)\n' total "$total"
((total <= 30000)) || err "release budget exceeded"
((total <= 35000)) || err "HARD STOP exceeded: no new features until a deletion pass"

# docs/design/experiments.md: the throwaway experiments stay within 5 000 code lines in total (generated code excluded, as above).
mapfile -t exp < <(git ls-files -co --exclude-standard -- 'experiments/*.rs' | grep -v '/generated/' || true)
if ((${#exp[@]})); then
  n=$(tokei -o json "${exp[@]}" | jq '.Total.code')
  printf 'budget: %-9s %6d / 5000\n' experiments "$n"
  ((n <= 5000)) || err "experiments exceed 5000 lines (docs/design/experiments.md)"
fi

# Every MEASURE marker carries non-empty id, date, ver and env. `id=<...>` is the format template, not a marker; what
# the agents of a demo wrote (`demos/*/*-example/`) holds no markers of this repository.
rc=0
markers=$(git -c submodule.recurse=false grep -nE --untracked '(<!--|//|#) MEASURE id=' -- . \
  ':!.loop' ':!.claude' ':!scripts/budget.sh' ':!scripts/budget-selftest.sh' ':!demos/*-example/*') || rc=$?
((rc <= 1)) || err "git grep failed with code $rc"
while IFS= read -r line; do
  [[ -z "$line" || "$line" =~ MEASURE\ id=\< ]] && continue
  bare=$(sed 's/"[^"]*"//g' <<<"$line") # keys inside quoted values (note="... ver=1") do not count
  for key in id ver env; do
    [[ "$bare" =~ [[:space:]]$key=[^[:space:]\"] ]] || err "MEASURE marker without $key: $line"
  done
  [[ "$bare" =~ [[:space:]]date=[0-9]{4}-[0-9]{2}-[0-9]{2} ]] || err "MEASURE marker without date: $line"
done <<<"$markers"

exit "$fail"

#!/bin/sh
# The last exec step: archive a deterministic handoff, close the epic and push its closed tracker state.
# `snapshot <run> <bead>` prints the full current handoff without closing, committing or pushing anything.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
mode=${1:-finish}
case "$mode" in
  finish) run=${LOOP_MANAGER_RUN:?}; bead=${LOOP_MANAGER_BEAD:?};;
  snapshot) run=${2:?run id}; bead=${3:?bead id};;
  *) echo "usage: report.sh [finish | snapshot <run> <bead>]" >&2; exit 1;;
esac
case "$run" in *[!a-zA-Z0-9_-]*|'') echo "report: invalid run id" >&2; exit 1;; esac
for tool in jq loop-manager br git; do command -v "$tool" >/dev/null; done
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
out=devloop/results/$run
artifacts=devloop/artifacts/$run
br comments "$bead" --json >"$tmp/comments.json"
br show "$bead" --json >"$tmp/bead.json"
jq --arg actor "loop-manager:$run:" 'map(select(.author | startswith($actor)))' "$tmp/comments.json" >"$tmp/run-comments.json"
# An epic can be run again. Only its first coder attempt knows where this run began.
base=$(jq -r --arg actor "loop-manager:$run:1" '[.[] | select(.author == $actor and (.text | startswith("CODER:")))
  | .text | split("\n")[1] // "" | select(startswith("BASE: ")) | ltrimstr("BASE: ")] | first // empty' "$tmp/run-comments.json")
# Prefer this run's reviewed revision to later tracker commits.
head=$(jq -r '[.[] | select(.text | startswith("REVIEW\n")) | .text
  | first(split("\n")[] | select(startswith("HEAD: "))) | ltrimstr("HEAD: ")] | last // empty' "$tmp/run-comments.json")
if [ -z "$head" ]; then
  if [ -f "$out/report.toon" ]; then
    # Only this top-level string is needed for a retry. An all-numeric revision can be quoted.
    head=$(jq -Rer 'select(startswith("code_head: ")) | ltrimstr("code_head: ")
      | if startswith("\"") then fromjson else . end' "$out/report.toon")
  else
    head=$(git rev-parse HEAD)
  fi
fi
: >"$tmp/numstat"
# Missing or stale handoff revisions leave the size delta unknown. Never substitute a later round's BASE.
case "$base$head" in *[!0-9a-fA-F]*) base=;; esac
if [ -n "$base" ] && git merge-base --is-ancestor "$base" "$head" 2>/dev/null; then
  # Counting a rename as deletion + addition also handles moves between crates and from product code to tests.
  git diff --no-ext-diff --no-textconv --no-renames --numstat -z "$base" "$head" -- \
    ':(glob)crates/*/**/*.rs' ':(exclude,glob)**/generated/**' >"$tmp/numstat"
else
  base=
fi
: >"$tmp/tests.txt"
source=target/devloop/runs/$run
[ -d "$source" ] || source=$artifacts
report_artifacts=$artifacts
comment_path=$out/comments.toon
if [ "$mode" = snapshot ]; then
  report_artifacts=$source
  [ -d "$source" ] || report_artifacts=
  comment_path=
fi
for digest in "$source"/tests.*/result.txt; do
  [ -f "$digest" ] || continue
  sed "s|^LOG: target/devloop/runs/$run/|LOG: $report_artifacts/|" "$digest" >>"$tmp/tests.txt"
  printf '\n---\n' >>"$tmp/tests.txt"
done
if [ "$mode" = finish ]; then
  mkdir -p "$out" "$artifacts"
  if [ -d "target/devloop/runs/$run" ]; then cp -R "target/devloop/runs/$run/." "$artifacts/"; fi
  # Already closed is a successful retry; preserve the assignee so the daemon can reconcile its close.
  if [ "$(jq -r '.[0].status' "$tmp/bead.json")" != closed ]; then
    br close "$bead" -r "Completed: review accepted; handoff $out/report.toon" -q >&2
  fi
  br show "$bead" --json >"$tmp/bead.json"
fi
jq -n --slurpfile comments "$tmp/comments.json" --slurpfile run_comments "$tmp/run-comments.json" --slurpfile bead "$tmp/bead.json" \
  --arg run "$run" --arg head "$head" --arg artifacts "$report_artifacts" --arg comment_path "$comment_path" \
  --arg base "$base" --rawfile numstat "$tmp/numstat" --rawfile tests "$tmp/tests.txt" -f "$here/report.jq" >"$tmp/report.json"
if [ "$mode" = snapshot ]; then loop-manager format <"$tmp/report.json"; exit 0; fi
loop-manager format <"$tmp/report.json" >"$tmp/report.toon"
loop-manager format <"$tmp/comments.json" >"$tmp/comments.toon"
mv "$tmp/report.toon" "$tmp/comments.toon" "$out/"
br sync --flush-only -q >&2
export GIT_TERMINAL_PROMPT=0
git add -- devloop/.beads "$out"
git diff --cached --quiet -- devloop/.beads "$out" || \
  git commit -q -m "chore(devloop): archive handoff for $run [$bead]" -- devloop/.beads "$out" >&2
git push -q >&2 || { echo "report: closed epic and handoff are local; git push failed" >&2; exit 75; }
# Exec outcomes retain only the last 4 KiB: give callers a bounded preview and the path to the complete report.
jq --arg report "$out/report.toon" '{run, epic, status, report: $report, artifacts, module_lines,
  summary: (.coder[:240]), review: (.review[:240]),
  question_count: (.questions | length), decision_count: (.decisions | length),
  questions: [.questions[:2][] | {key: .key[:80], text: .text[:160]}],
  decisions: [.decisions[:2][] | {key: .key[:80], text: .text[:160]}]
}' "$tmp/report.json" >"$tmp/preview.json"
loop-manager format <"$tmp/preview.json" >"$tmp/preview.toon"
if [ "$(wc -c <"$tmp/preview.toon")" -gt 3500 ]; then
  jq 'del(.module_lines)' "$tmp/preview.json" >"$tmp/no-lines.json"
  loop-manager format <"$tmp/no-lines.json" >"$tmp/preview.toon"
fi
if [ "$(wc -c <"$tmp/preview.toon")" -gt 3500 ]; then
  jq '{run, epic, status, report, artifacts, question_count, decision_count}' "$tmp/preview.json" >"$tmp/small.json"
  loop-manager format <"$tmp/small.json" >"$tmp/preview.toon"
fi
cat "$tmp/preview.toon"

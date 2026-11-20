# Comments arrive oldest first. Repeated concerns replace their previous text; only a later ANSWER resolves one.
def concern:
  (.text | split("\n")[0] | capture("^(?<kind>QUESTION|DECISION|ANSWER): (?<key>[^\r\n]+)$")) as $header
  | {kind: $header.kind, key: $header.key, text, author, created_at};
def module_lines:
  $numstat | split("\u0000")
  | map(capture("^(?<added>[0-9]+)\t(?<removed>[0-9]+)\tcrates/(?<module>[^/]+)/(?<path>.*)$"; "s")
      | .lines = ((.added | tonumber) - (.removed | tonumber))
      | .kind = (if .module == "loop-manager-testkit" or .module == "loop-manager-acceptance"
                     or (.path | test("(^|/)tests(\\.rs$|/)")) then "tests" else "code" end))
  | group_by(.module)
  | map(reduce .[] as $file ({module: .[0].module, code: 0, tests: 0}; .[$file.kind] += $file.lines));
($comments[0] | map(select(.text | type == "string"))) as $all
| (reduce ($all[] | concern) as $item ({};
    if $item.kind == "ANSWER" then del(.[$item.key]) else .[$item.key] = $item end)
   | to_entries | map(.value)) as $open
| {
    run: $run,
    epic: $bead[0][0].id,
    title: $bead[0][0].title,
    status: $bead[0][0].status,
    code_head: $head,
    code_base: (if $base != "" then $base else null end),
    module_lines: (if $base != "" then module_lines else null end),
    artifacts: (if $artifacts == "" then null else $artifacts end),
    comments: (if $comment_path == "" then null else $comment_path end),
    coder: ([$run_comments[0][] | select(.text | startswith("CODER:"))] | last | .text // ""),
    review: ([$run_comments[0][] | select(.text | startswith("REVIEW\n"))] | last | .text // ""),
    questions: [$open[] | select(.kind == "QUESTION") | del(.kind)],
    decisions: [$open[] | select(.kind == "DECISION") | del(.kind)],
    tests: ($tests | split("\n---\n") | map(select(length > 0)))
  }

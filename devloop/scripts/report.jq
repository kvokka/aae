# Comments arrive oldest first. Repeated concerns replace their previous text; only a later ANSWER resolves one.
def concern:
  (.text | split("\n")[0] | capture("^(?<kind>QUESTION|DECISION|ANSWER): (?<key>[^\r\n]+)$")) as $header
  | {kind: $header.kind, key: $header.key, text, author, created_at};
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
    artifacts: (if $artifacts == "" then null else $artifacts end),
    comments: (if $comment_path == "" then null else $comment_path end),
    coder: ([$all[] | select(.text | startswith("CODER:"))] | last | .text // ""),
    review: ([$all[] | select(.text | startswith("REVIEW\n"))] | last | .text // ""),
    questions: [$open[] | select(.kind == "QUESTION") | del(.kind)],
    decisions: [$open[] | select(.kind == "DECISION") | del(.kind)],
    tests: ($tests | split("\n---\n") | map(select(length > 0)))
  }

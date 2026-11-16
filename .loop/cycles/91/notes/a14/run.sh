#!/usr/bin/env bash
# A14 p.4: acceptance --ci in parallel with nextest --workspace, N times.
cd "$(git rev-parse --show-toplevel)"
d=.loop/cycles/91/notes/a14
for i in 1 2 3 4 5; do
  s=$(date +%s)
  cargo run -q --locked -p aae-acceptance -- --ci > $d/acc-$i.log 2>&1 & a=$!
  cargo nextest run --workspace --no-fail-fast --status-level fail --final-status-level fail > $d/nt-$i.log 2>&1 & n=$!
  wait $a; ra=$?; wait $n; rn=$?
  echo "run $i: acceptance=$ra nextest=$rn $(( $(date +%s) - s ))s" >> $d/summary.txt
done

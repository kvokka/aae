#!/bin/sh
# The user's check of the result, run by the formula `smoke` in this directory once the planner passes. Exit 0: the
# backend stores and serves a score and the frontend has the game, as REQUEST.md says.
set -eu
backend=$(cd ../backend && pwd)
page=$(cd ../frontend && pwd)/index.html
port=$((20000 + $$ % 10000))
cd "$backend"
PORT=$port python3 server.py &
server=$!
trap 'kill $server 2>/dev/null' EXIT
url=http://127.0.0.1:$port/scores
tries=0
until curl -fsS "$url" >/dev/null 2>&1; do
  tries=$((tries + 1))
  [ "$tries" -lt 50 ] || { echo "the backend does not answer on port $port"; exit 1; }
  sleep 0.2
done
curl -fsS -X POST -H 'Content-Type: text/plain' --data 'smoke 4242' "$url"
grep -Eq '^smoke[[:space:]]+4242[[:space:]]*$' scores.txt || { echo "scores.txt does not hold the score"; exit 1; }
curl -fsS "$url" | grep -Eq '^smoke[[:space:]]+4242' || { echo "GET /scores does not list the score"; exit 1; }
for what in '<canvas' 'ArrowUp' '/scores'; do
  grep -q "$what" "$page" || { echo "$page has no $what"; exit 1; }
done
echo "smoke test passed: the backend stored and served a score, the frontend has the game"

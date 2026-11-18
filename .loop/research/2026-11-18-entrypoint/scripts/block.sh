#!/bin/bash
# usage: block.sh <label|none> <ncalls> <nwords> <model> <outdir>
LABEL=$1; N=$2; W=$3; M=$4; OUT=$5
S=/tmp/tmp.rQllDKosDj; CL=$(mise which claude)
mkdir -p $OUT; cd $S/cwd
EXTRA=(); [ "$LABEL" != none ] && EXTRA=(CLAUDE_CODE_ENTRYPOINT=$LABEL)
for i in $(seq 1 $N); do
  P=$OUT/prompt_$i.txt; python3 -I $S/scripts/mkprompt.py $W $P
  env -i PATH=/usr/bin:/bin HOME=$HOME ${PROXY:+ANTHROPIC_BASE_URL=$PROXY} DISABLE_PROMPT_CACHING=1 "${EXTRA[@]}" \
    timeout 300 $CL -p --output-format stream-json --verbose --model $M --tools "" --setting-sources local \
    --strict-mcp-config --no-session-persistence --effort low --max-turns 1 < $P > $OUT/call_$i.jsonl 2> $OUT/call_$i.err
  echo "$(date +%T) call $i rc=$?" >> $OUT/calls.log
  gzip -q $P
done

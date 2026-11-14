#!/usr/bin/env bash
# Spec §2.1 item 6: the core knows no harness and no tracker by name; what it needs comes through the driver
# capabilities and the tracker interface. Checks the code of `aae-core` outside its tests: `src/tests/` and
# whatever follows the first top-level `#[cfg(test)]` or `#[test]` of a file.
set -euo pipefail
cd "$(dirname "$0")/.."
names='\b(claude|codex|br|max_sessions|instance_idle_s|BEADS_DIR)\b|\.beads\b'
found=$(find crates/aae-core/src -name '*.rs' -not -path '*/src/tests/*' -print0 | sort -z |
  xargs -0 awk 'FNR == 1 { on = 1 } /^#\[(cfg\(test\)|test)\]/ { on = 0 } on { print FILENAME ":" FNR ": " $0 }' |
  grep -E "$names" || true)
if [[ -n $found ]]; then
  echo "harness or tracker names in aae-core (spec §2.1 item 6):"
  echo "$found"
  exit 1
fi

#!/usr/bin/env bash
# Decision 43: the core knows no harness and no tracker by name; what it needs comes through the driver
# capabilities and the tracker interface, in any letter case. Checks the code of the core (`aae-contract`, `aae-core`)
# outside its tests: `src/tests/`, every `tests.rs` (the tests of a module, in a file of their own) and the top-level
# item under a `#[cfg(test)]` or `#[test]` (up to its closing `}` at column 0 when it opens a block).
set -euo pipefail
cd "$(dirname "$0")/.."
# The harness names come from the adapter table of the daemon, the one place that lists them.
harnesses=$(grep -o 'aae_[a-z0-9]*::ADAPTER\b' crates/aae/src/harness.rs | sed 's/^aae_//; s/::ADAPTER$//' | paste -sd '|')
if [[ -z $harnesses ]]; then
  echo "no adapter found in the table of crates/aae/src/harness.rs"
  exit 1
fi
names="\\b($harnesses|br|max_sessions|instance_idle_s|beads_dir)\\b|\\.beads\\b"
found=$(find crates/aae-contract/src crates/aae-core/src -name '*.rs' -not -path '*/src/tests/*' -not -name tests.rs -print0 |
  sort -z |
  xargs -0 awk '
    FNR == 1 { on = 1; item = 0 }
    item && /^#\[/ { next }
    item { item = 0; if (/\{$/) on = 0; next }
    /^#\[(cfg\(test\)|test)\]/ { item = 1; next }
    !on { if (/^}/) on = 1; next }
    { print FILENAME ":" FNR ": " $0 }' |
  grep -iE "$names" || true)
if [[ -n $found ]]; then
  echo "harness or tracker names in the core (decision 43):"
  echo "$found"
  exit 1
fi
# The adapters build without the engine: what they share with it is `aae-contract`. Their tests may use it.
for crate in aae-claude aae-codex aae-br aae-addons; do
  if cargo tree -q -e normal,build --prefix none -p "$crate" | grep -q '^aae-core '; then
    echo "$crate depends on aae-core (decision 43): $(cargo tree -q -e normal,build -i aae-core -p "$crate" | paste -sd ' ')"
    exit 1
  fi
done

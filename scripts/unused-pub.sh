#!/usr/bin/env bash
# Spec §15, criterion 23: every `pub` item is used somewhere in the workspace besides its own definition and
# comments; `unreachable_pub` (Cargo.toml) already denies `pub` items no crate root exports.
# shortcut: matches by name, so an unused item that shares its name with a used one passes; a real cross-crate
# analysis (rust-analyzer or rustdoc JSON) is the upgrade if such a miss is ever found.
set -euo pipefail
cd "$(dirname "$0")/.."
found=0
while IFS=: read -r file line name; do
  uses=$(grep -rwn --include='*.rs' -- "$name" crates examples 2>/dev/null | grep -v "^$file:$line:" | grep -vE '^[^:]+:[0-9]+:\s*//' | head -1 || true)
  if [[ -z $uses ]]; then
    echo "unused public item: $name ($file:$line)"
    found=1
  fi
done < <(grep -rnE --include='*.rs' '^\s*pub (async )?(fn|struct|enum|trait|const|static|type|mod) [A-Za-z_][A-Za-z0-9_]*' crates/*/src \
  | sed -E 's/^([^:]+):([0-9]+):\s*pub (async )?(fn|struct|enum|trait|const|static|type|mod) ([A-Za-z_][A-Za-z0-9_]*).*/\1:\2:\5/')
exit $found

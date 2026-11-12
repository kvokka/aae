#!/usr/bin/env bash
# Self-test for scripts/budget.sh: a clean tree passes, each planted violation fails.
set -euo pipefail
script="$(cd "$(dirname "$0")" && pwd)/budget.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fresh() {
  rm -rf "$tmp/r" && mkdir -p "$tmp/r/crates/aae/src" "$tmp/r/docs" && cd "$tmp/r" && git init -q
  echo 'fn main() {}' >crates/aae/src/main.rs
  echo '<!-- MEASURE id=x date=2026-11-12 ver=0.1.0 env=linux note="n" -->' >docs/READINESS.md
}
expect() { # expect pass|fail <label>
  if "$script" "$tmp/r" >/dev/null 2>&1; then got=pass; else got=fail; fi
  [[ "$got" == "$1" ]] || { echo "budget-selftest: $2: expected $1, got $got" >&2; exit 1; }
}

fresh; expect pass "clean tree"
fresh; seq 401 | sed 's/.*/const _: u8 = 0;/' >crates/aae/src/big.rs; expect fail "file over 400 lines"
fresh; seq 401 | sed 's/.*/const _: u8 = 0;/' >crates/aae/src/tests.rs; expect pass "test file is exempt"
fresh; echo '// MEASURE id=y date=2026-11-12 env=linux' >crates/aae/src/m.rs; expect fail "marker without ver"
fresh; mkdir -p crates/other/src && echo 'fn f() {}' >crates/other/src/lib.rs; expect fail "unbudgeted crate"
fresh; rm docs/READINESS.md; expect fail "missing readiness file"
echo "budget-selftest: ok"

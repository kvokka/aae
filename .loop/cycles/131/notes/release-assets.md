# Release assets: mise github backend, cargo-binstall, cargo publish --workspace

Local: mise 2026.11.5, cargo 1.100.0. Sources read from raw.githubusercontent.com (WebFetch was blocked; docs.md and source were fetched with curl).

## 1. mise `github:` backend
Sources: https://github.com/jdx/mise/blob/main/docs/dev-tools/backends/github.md ("How mise picks an asset"),
https://github.com/jdx/mise/blob/main/src/backend/asset_matcher.rs

- No `asset_pattern` -> autodetect: score every asset; rewards OS + arch + libc match, known archive formats, names starting with the repo name; penalises checksum/signature/.app/debug; highest wins, tie -> shortest name.
- OS regexes (asset_matcher.rs ~l.125-130): linux; `darwin|mac|macos|macosx|osx`. "apple" is NOT recognised, but `darwin` in `*-apple-darwin` is enough.
- Arch regexes (~l.147-151): `x86_64|x86-64|x64|amd64`; `aarch64|aarch_64|arm64|arm_64`. Boundaries `\b` or `_`, so `-x86_64-` and `-aarch64-` match.
- libc: musl token recognised; on a glibc Linux host it prefers gnu/glibc names but "takes one for the other libc when that is the best match" (docs `libc` option). Only musl assets exist -> musl is picked. (The code had musl +10 / gnu -10 in one branch, l.650, as a static-link preference; not needed here.)
- `.sha256` (also .sha256sum(s), .asc, .sig, .sbom...) are classified as metadata and penalised (l.727-775): never picked over the real archive.
- `.tar.gz` extracted; single executable found in extracted tree (bin_path/strip_components optional). Binary at root: fine.
- Checksums: verifies GitHub-reported asset digest, or a checksum file in the same release. `find_checksum_for` (l.1133) looks for `<asset>.sha256` (also `.sha256sum`, `.SHA256`, ...), then `sha256sums.txt`/`checksums.txt`. Content: either bare `<hash>` or `<hash>  <filename>`; if two-column, filename must equal the archive name (`loop-manager-<target>.tar.gz`), so produce it with `sha256sum loop-manager-<target>.tar.gz > loop-manager-<target>.tar.gz.sha256` run in the dir of the archive.
- Verdict: proposed names work on all four platforms with no options. Fallback if ever needed: `asset_pattern = "loop-manager-*-linux-musl.tar.gz"` etc. (not needed).
- Untested: no network install run from this sandbox (needs the real release).

## 2. cargo-binstall
Source: https://github.com/cargo-bins/cargo-binstall/blob/main/SUPPORT.md and crates/binstalk-bins/src/lib.rs (`infer_bin_dir_template`)

- Documented default: `pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }{ archive-suffix }"`, `bin-dir = "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"`, `pkg-fmt = "tgz"`.
- In practice it tries ALL combos: 10 filename forms (incl. versionless `{ name }-{ target }{ archive-suffix }` and `{ name }_{ target }...`) x paths for GitHub (`{ repo }/releases/download/{ version }/`, `.../v{ version }/`, `{ name }-v{ version }/`, ...), and tries all archive formats.
- bin-dir: after extraction tries dirs `{name}-{target}-v{version}`, ..., `{name}-{target}`, `{name}-{version}`, `{name}-v{version}`, `{name}`; first that exists wins; if none exists it falls back to `{ bin }{ binary-ext }` at the archive root.
- => `https://github.com/kvokka/loop-manager/releases/download/v<version>/loop-manager-<target>.tar.gz` with binary at root IS found with no metadata (needs `repository = "https://github.com/kvokka/loop-manager"` in Cargo.toml of the crate). Caveat: do not put a top-level directory named `loop-manager` in the archive (the file at root is fine; a file named `loop-manager` is not a dir).
- Explicit snippet if one wants to pin it (optional, equivalent):
```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }{ archive-suffix }"
bin-dir = "{ bin }{ binary-ext }"
pkg-fmt = "tgz"
```
- Untested: no network run of binstall here.

## 3. `cargo publish --workspace`
Sources: `cargo publish --help` locally (cargo 1.100: `--workspace`, `--exclude <SPEC>`, `-p`, `--no-verify`);
https://github.com/rust-lang/cargo/issues/14789 ; https://github.com/driftsys/ridl/pull/493 ; https://github.com/OffeneDatenmodellierung/open-spec-conform/pull/24

- Exists on stable (shown by local `--help`). From my knowledge (not re-verified by source here) it was stabilised in 1.90, packages the whole set, topologically orders by path-dependency and waits for each crate to appear in the index.
- Already-published: real run fails with `error: crate <name>@<ver> already exists on crates.io index` (exit 101); dry-run only prints `warning: crate X@V already exists on crates.io index` (issue #14789). I did not find a skip-existing flag (cargo help lists none); only `--exclude <SPEC>`.
- Re-run pattern: (a) publish per crate in dependency order, `cargo publish --locked -p <crate>`, and treat output containing `already exists on crates.io index` as success (ridl#493 reads cargo's own error rather than querying the crates.io API, which 403s without a User-Agent and lags behind publish); or (b) pre-check the sparse index/API for name@version (needs User-Agent) and `--exclude` those. For a single publishable crate (workspace with one non-`publish = false` member): `cargo publish || grep-the-error` is enough; capture output with `tee` and match the text, else fail.

#!/bin/sh
set -eu
addon=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'find "$tmp" -delete' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.org GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.org
git init -q "$tmp/work"
cd "$tmp/work"
# Exercise the configured Git hook together with a repository's own commit-msg hook.
printf '#!/bin/sh\ntouch .git/local-hook-ran\n' >.git/hooks/commit-msg
chmod +x .git/hooks/commit-msg
export PATH="$addon/bin:$PATH"
export LOOP_MANAGER_ADDON_DIR="$addon"
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0="$addon/gitconfig"
decision=$(jq -n --arg dir "$PWD" '{data:{workdir:$dir}}' | "$addon/hooks/prepare")
LOOP_MANAGER_COMMIT_GIT_DIR=$(echo "$decision" | jq -er '.env.LOOP_MANAGER_COMMIT_GIT_DIR')
export LOOP_MANAGER_COMMIT_GIT_DIR
commit() { git commit -q --allow-empty -m "$1"; }
reject() {
  if commit "$1"; then echo "unexpectedly accepted: $1" >&2; exit 1; fi
}
unset LOOP_MANAGER_BEAD
commit 'docs: describe the parser'
test -f .git/local-hook-ran
reject 'update docs'
export LOOP_MANAGER_BEAD=dev-3.1
commit 'feat(api)!: change the response [dev-3.1]'
reject 'fix: missing bead'
reject 'fix: wrong bead [dev-3x1]'
reject 'fix:  [dev-3.1]'
git init -q "$tmp/fixture"
cd "$tmp/fixture"
commit 'fixture setup'

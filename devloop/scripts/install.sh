#!/bin/sh
# Promote the existing dev artifact without modifying a running executable in place.
set -eu
if [ -n "${LOOP_MANAGER_RUN:-}${DEVLOOP_TEST_ENV:-}" ]; then
  echo "This is an operator command for the working devloop. Use isolated tests inside a run." >&2
  exit 1
fi
metadata=$(cargo metadata --locked --no-deps --format-version 1)
target=$(printf '%s\n' "$metadata" | jq -er '.target_directory')
artifact="$target/debug/loop-manager"
if [ ! -f "$artifact" ] || [ ! -x "$artifact" ]; then
  echo "Missing executable $artifact. Build it with cargo build --locked -p loop-manager." >&2
  exit 1
fi

bin="$HOME/.local/opt/loop-manager-dev/bin"
mkdir -p "$bin"
tmp=$(mktemp "$bin/.loop-manager.XXXXXX")
trap 'rm -f "$tmp"' EXIT
install -m 0755 "$artifact" "$tmp"
mv -f "$tmp" "$bin/loop-manager"
printf 'Installed %s at %s/loop-manager\n' "$artifact" "$bin"

# Publish this daemon's name to local run-card clients, including clients started outside this checkout.
case "${XDG_CONFIG_HOME:-}" in
  /*) config_dir=$XDG_CONFIG_HOME ;;
  *) config_dir="$HOME/.config" ;;
esac
mkdir -p "$config_dir/loop-manager/sockets"
ln -sfn "$HOME/.local/state/loop-manager-dev/loop-manager/loop-manager.sock" "$config_dir/loop-manager/sockets/devloop"

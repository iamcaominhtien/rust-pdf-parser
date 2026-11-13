#!/usr/bin/env bash
# Stop: if Claude edited Rust code this turn, run the CI lint gate before it hands back.
#   cargo fmt  ->  cargo clippy --fix  ->  cargo clippy --all-targets -- -D warnings
# Warnings that clippy cannot fix by itself go back to Claude (exit 2), once per turn.
set -u
root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
mark="$root/target/.claude-rust-edited"
[ -f "$mark" ] || exit 0
active=$(jq -r '.stop_hook_active // false' 2>/dev/null || echo false)
cd "$root" || exit 0

cargo fmt --all >/dev/null 2>&1
cargo clippy --all-targets --fix --allow-dirty --allow-staged --quiet >/dev/null 2>&1
if out=$(cargo clippy --all-targets --quiet -- -D warnings 2>&1); then
  rm -f "$mark"
  exit 0
fi
if [ "$active" = "true" ]; then
  # Already sent back once this turn: do not loop; leave the mark for the next turn.
  echo '{"systemMessage":"clippy still reports warnings; run cargo clippy --all-targets -- -D warnings"}'
  exit 0
fi
echo "cargo clippy --all-targets -- -D warnings fails (CI runs the same check). Fix these:" >&2
echo "$out" | grep -vE '^\s*(Checking|Compiling|Finished)' | head -60 >&2
exit 2

#!/usr/bin/env bash
# PostToolUse (Edit|Write|MultiEdit): format or check the file Claude just changed.
#  - .rs   : rustfmt (rustfmt.toml), and mark the tree for clippy at the end of the turn
#  - .js   : syntax check with node (no reformatting: the renderer's JS keeps its compact style)
#  - .json : must parse
#  - .sh   : bash -n
# A problem is reported back to Claude with exit code 2 (stderr), so it fixes it right away.
set -u
root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
f=$(jq -r '.tool_input.file_path // .tool_response.filePath // empty')
[[ -n "$f" && -f "$f" ]] || exit 0
case "$f" in "$root"/*) ;; *) exit 0 ;; esac
case "$f" in */target/*|*/.claude/worktrees/*) exit 0 ;; esac

case "$f" in
  *.rs)
    mkdir -p "$root/target" && touch "$root/target/.claude-rust-edited"
    if ! out=$(cd "$root" && rustfmt --edition 2024 "$f" 2>&1); then
      echo "rustfmt failed on $f:" >&2
      echo "$out" | head -40 >&2
      exit 2
    fi ;;
  *.js)
    if ! out=$(node --check "$f" 2>&1); then
      echo "JavaScript syntax error in $f:" >&2
      echo "$out" | head -20 >&2
      exit 2
    fi ;;
  *.json)
    if ! out=$(jq empty "$f" 2>&1); then
      echo "Invalid JSON in $f: $out" >&2
      exit 2
    fi ;;
  *.sh)
    if ! out=$(bash -n "$f" 2>&1); then
      echo "Shell syntax error in $f:" >&2
      echo "$out" >&2
      exit 2
    fi ;;
esac
exit 0

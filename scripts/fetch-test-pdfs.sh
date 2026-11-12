#!/usr/bin/env bash
# Downloads the arXiv papers listed in tests/data/papers.txt that are not in
# tests/data yet, at the pinned version, and checks their sha256.
# arXiv asks automated clients to go slowly, so downloads are spaced out.
set -euo pipefail
cd "$(dirname "$0")/../tests/data"
first=1
while read -r id sum; do
  case "$id" in ''|'#'*) continue ;; esac
  file="$id.pdf"
  if [ -f "$file" ] && echo "$sum  $file" | sha256sum -c --status; then
    continue
  fi
  [ "$first" = 1 ] || sleep 3
  first=0
  echo "downloading $id"
  curl -fsSL --retry 3 --retry-delay 5 -A "paper2html-tests" -o "$file.part" "https://arxiv.org/pdf/$id"
  if ! echo "$sum  $file.part" | sha256sum -c --status; then
    echo "checksum mismatch for $id" >&2
    rm -f "$file.part"
    exit 1
  fi
  mv "$file.part" "$file"
done < papers.txt
echo "test papers ready"

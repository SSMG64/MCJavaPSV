#!/usr/bin/env bash
# Fails when the repository contains material that must never be published:
# Mojang files, decompiled sources, or files derived from a player's install.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

forbidden_ext='\.(jar|class|java|nbt|mcmeta|fsh|vsh|glsl|ogg|ico|vpk|self|velf|sfo)$'
forbidden_path='(^|/)(net/minecraft|com/mojang|assets/minecraft|data/minecraft|\.minecraft)(/|$)'
allowed='^crates/psv-vpk/tests/data/golden-[a-z-]+\.sfo$'

status=0
while IFS= read -r file; do
  if [[ "$file" =~ $allowed ]]; then
    continue
  fi
  if [[ "$file" =~ $forbidden_ext ]] || [[ "$file" =~ $forbidden_path ]]; then
    echo "forbidden file: $file" >&2
    status=1
  fi
  case "$file" in
    *.png)
      case "$file" in
        packaging/*|docs/media/*) ;;
        *) echo "image outside packaging/ or docs/media/: $file" >&2; status=1 ;;
      esac
      ;;
  esac
done < <(git ls-files)

if [ "$status" -ne 0 ]; then
  echo "See docs/LEGAL.md: this repository must not contain Mojang material." >&2
fi
exit "$status"

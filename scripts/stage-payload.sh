#!/usr/bin/env bash
set -euo pipefail

horizon="${1:?usage: stage-payload.sh <horizon-checkout> <payload-dir> [eboot.bin]}"
payload="${2:?usage: stage-payload.sh <horizon-checkout> <payload-dir> [eboot.bin]}"
eboot="${3:-}"

tool="$horizon/target/release/horizon-prepare"
[ -f "$tool.exe" ] && tool="$tool.exe"
[ -f "$tool" ] || { echo "horizon-prepare was not built at $tool" >&2; exit 1; }

mkdir -p "$payload"
cp "$tool" "$payload/"
cp "$horizon/research-lock.json" "$payload/"
cp "$horizon/LICENSE" "$payload/HORIZON-LICENSE"

if [ -n "$eboot" ] && [ -f "$eboot" ]; then
  cp "$eboot" "$payload/eboot.bin"
else
  echo "no eboot.bin supplied; the payload has none" >&2
fi
echo "payload staged in $payload"

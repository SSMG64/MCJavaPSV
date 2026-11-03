#!/usr/bin/env bash
set -euo pipefail

dest="${1:?usage: fetch-horizon.sh <destination>}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rev="$(tr -d '[:space:]' < "$root/packaging/horizon-rev.txt")"

mkdir -p "$dest"
cd "$dest"
if [ ! -d .git ]; then
  git init --quiet
  git remote add origin https://github.com/ludovic111/horizon.git
fi
git fetch --quiet --depth 1 origin "$rev"
git checkout --quiet --detach FETCH_HEAD
echo "Horizon checked out at $(git rev-parse HEAD)"

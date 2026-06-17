#!/usr/bin/env bash
set -euo pipefail

repo_url="${APIS_GURU_OPENAPI_DIRECTORY_REPO:-https://github.com/APIs-guru/openapi-directory.git}"
target_dir="${APIS_GURU_OPENAPI_DIRECTORY_PATH:-/var/lib/capability-core/openapi-directory}"

mkdir -p "$(dirname "$target_dir")"

if [ -d "$target_dir/.git" ]; then
  git -C "$target_dir" fetch --depth=1 origin master
  git -C "$target_dir" checkout -q master
  git -C "$target_dir" pull --ff-only origin master
else
  git clone --depth=1 "$repo_url" "$target_dir"
fi

git -C "$target_dir" rev-parse HEAD

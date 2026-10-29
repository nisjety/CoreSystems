#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
env_file="deploy/compose/.env"
if [[ ! -f "$env_file" ]]; then
  echo "Missing $env_file. Copy deploy/compose/.env.example and replace every placeholder." >&2
  exit 1
fi

export SOURCE_REVISION="${SOURCE_REVISION:-$(git rev-parse HEAD)}"
export BUILD_DATE="${BUILD_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

docker compose --env-file "$env_file" -f deploy/compose/docker-compose.yml up --build "$@"

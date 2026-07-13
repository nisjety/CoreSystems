#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export SOURCE_REVISION="${SOURCE_REVISION:-$(git -C "$ROOT_DIR" rev-parse HEAD)}"
export BUILD_DATE="${BUILD_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

compose_files=(
  -f "$ROOT_DIR/deploy/docker-compose.yml"
)

production="${MODEL_PLANE_PRODUCTION:-0}"

if [[ "$production" != "1" && "${MODEL_PLANE_DEV:-1}" != "0" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.override.yml")
fi

if [[ "${MODEL_PLANE_BRIDGES:-0}" == "1" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.bridges.yml")
fi

# The production layer must be last so its port resets, required credentials
# and least-privilege runtime settings cannot be undone by a development or
# optional service overlay.
if [[ "$production" == "1" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.production.yml")
fi

exec docker compose --env-file "$ROOT_DIR/.env" "${compose_files[@]}" "$@"

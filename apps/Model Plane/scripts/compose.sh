#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

compose_files=(
  -f "$ROOT_DIR/deploy/docker-compose.yml"
)

if [[ "${MODEL_PLANE_DEV:-1}" != "0" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.override.yml")
fi

if [[ "${MODEL_PLANE_BRIDGES:-0}" == "1" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.bridges.yml")
fi

exec docker compose --env-file "$ROOT_DIR/.env" "${compose_files[@]}" "$@"

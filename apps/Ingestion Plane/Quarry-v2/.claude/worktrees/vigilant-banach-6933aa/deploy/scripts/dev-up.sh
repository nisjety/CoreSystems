#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
docker compose -f deploy/compose/docker-compose.yml up --build "$@"

#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir"

echo "Stopping CoreSystem Infra Plane containers only. No volumes or other-plane resources are removed."
docker compose --env-file .env.local down --remove-orphans

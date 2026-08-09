#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir"

if [[ ! -f .env.local ]]; then
  cp .env.example .env.local
  chmod 600 .env.local
  echo "Created .env.local from .env.example. Set CORE_INFRA_OPERATOR_TOKEN to enable the operator endpoint."
fi

docker compose --env-file .env.local up -d --build
docker compose --env-file .env.local ps

core_infra_port="$(awk -F= '/^CORE_INFRA_HTTP_PORT=/{print $2}' .env.local | tail -n 1)"
core_infra_port="${core_infra_port:-8090}"

echo "Waiting for CoreSystem Infra Plane on 127.0.0.1:${core_infra_port}..."
for attempt in {1..20}; do
  if curl --fail --silent --show-error "http://127.0.0.1:${core_infra_port}/infra/health" >/dev/null; then
    echo "CoreSystem Infra Plane is ready at http://127.0.0.1:${core_infra_port}"
    exit 0
  fi
  sleep 1
done

echo "CoreSystem Infra Plane did not become ready. Inspect: docker compose logs --tail=100"
exit 1

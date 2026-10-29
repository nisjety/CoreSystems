#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
compose="$root/docker-compose.yml"

if [[ -e "$root/.env" || -e "$root/.env.example" ]]; then
  printf 'service env contract: Control Plane root .env files must not exist; use per-core files and the local runner\n' >&2
  exit 1
fi
if [[ ! -x "$root/scripts/run-control-plane.sh" ]]; then
  printf 'service env contract: scripts/run-control-plane.sh must be executable\n' >&2
  exit 1
fi
if ! grep -Fq 'database_value_from_urls' "$root/scripts/run-control-plane.sh"; then
  printf 'service env contract: local runner must reuse the service-local DATABASE_URL password for Postgres\n' >&2
  exit 1
fi
if ! grep -Fq 'network inspect inter-plane-bus' "$root/scripts/run-control-plane.sh"; then
  printf 'service env contract: local runner must provision the external inter-plane-bus network when starting\n' >&2
  exit 1
fi

services=(auth-core user-core org-core audit-core billing-core session-core)
for service in "${services[@]}"; do
  for file in "$root/$service/.env" "$root/$service/.env.example"; do
    if [[ ! -f "$file" ]]; then
      printf 'service env contract: missing %s\n' "$file" >&2
      exit 1
    fi
  done
  if ! grep -Fq "./$service/.env" "$compose"; then
    printf 'service env contract: compose must load ./%s/.env\n' "$service" >&2
    exit 1
  fi
done

check_keys() {
  local service=$1
  shift
  for key in "$@"; do
    if ! grep -Eq "^${key}=" "$root/$service/.env.example"; then
      printf 'service env contract: %s/.env.example must declare %s\n' "$service" "$key" >&2
      exit 1
    fi
  done
}

check_keys audit-core DATABASE_URL NATS_URL NATS_USER NATS_PASSWORD HTTP_PORT AUDIT_CORE_SERVICE_CREDENTIALS
check_keys billing-core DATABASE_URL NATS_URL NATS_USER NATS_PASSWORD HTTP_PORT BILLING_CORE_SERVICE_CREDENTIALS ORG_CORE_SERVICE_TOKEN
check_keys session-core DATABASE_HOST DATABASE_PORT DATABASE_USER DATABASE_PASSWORD DATABASE_NAME NATS_LOCAL_URL NATS_USER NATS_PASSWORD SERVER_HTTP_PORT USER_CORE_SERVICE_TOKEN ORG_CORE_SERVICE_TOKEN BILLING_CORE_SERVICE_TOKEN

printf 'PASS: six Control services have independent .env contracts\n'

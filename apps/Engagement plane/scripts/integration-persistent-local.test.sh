#!/bin/sh
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
compose_file="$root/docker-compose.integration-persistent.local.yml"
test -f "$compose_file"

fixture_dir=$(mktemp -d)
trap 'rm -f "$fixture_dir/google_secret"; rmdir "$fixture_dir"' EXIT INT TERM
printf '%s' synthetic-secret > "$fixture_dir/google_secret"
chmod 600 "$fixture_dir/google_secret"

env \
  INTEGRATION_LOCAL_HOST_PORT=3026 \
  INTEGRATION_LOCAL_DB_PASSWORD=synthetic-db-password \
  INTEGRATION_LOCAL_GOOGLE_SECRET_FILE="$fixture_dir/google_secret" \
  INTEGRATION_LOCAL_INTERNAL_API_KEY=synthetic-internal-key \
  INTEGRATION_LOCAL_AUTH_CORE_INTERNAL_API_KEY=synthetic-auth-key \
  INTEGRATION_LOCAL_ORG_SERVICE_TOKEN=synthetic-org-service-token-123456789 \
  INTEGRATION_LOCAL_BILLING_SERVICE_TOKEN=synthetic-billing-service-token-123456789 \
  INTEGRATION_LOCAL_AUDIT_SERVICE_TOKEN=synthetic-audit-service-token-123456789 \
  INTEGRATION_LOCAL_VAULT_KEY=synthetic-vault-key \
  INTEGRATION_LOCAL_GOOGLE_CLIENT_ID=synthetic-client-id \
  INTEGRATION_LOCAL_WRITE_ATTESTATION_KEYS_JSON='[]' \
  docker compose -f "$compose_file" config --format json |
  jq -e '
    .services."integration-api".environment.GOOGLE_CLIENT_SECRET_FILE == "/run/secrets/google_client_secret"
    and .services."integration-api".environment.GOOGLE_CLIENT_SECRET == ""
    and .services."integration-api".environment.INTEGRATION_PUBLIC_BASE_URL == "http://localhost:3026"
    and .services."integration-api".environment.INTEGRATION_ALLOW_IN_MEMORY_STORE == null
    and .services."integration-api".environment.ALLOW_LEGACY_TENANT_KEY == null
    and .services."integration-api".environment.NATS_ENABLED == "false"
    and .services."integration-api".ports == null
    and .services."integration-api".depends_on."integration-postgres".condition == "service_healthy"
    and (.services."integration-api".networks | has("inter-plane-bus"))
    and (.services."integration-postgres".volumes | length) == 1
  ' >/dev/null

env \
  INTEGRATION_LOCAL_HOST_PORT=3026 \
  INTEGRATION_LOCAL_DB_PASSWORD=synthetic-db-password \
  INTEGRATION_LOCAL_GOOGLE_SECRET_FILE="$fixture_dir/google_secret" \
  INTEGRATION_LOCAL_INTERNAL_API_KEY=synthetic-internal-key \
  INTEGRATION_LOCAL_AUTH_CORE_INTERNAL_API_KEY=synthetic-auth-key \
  INTEGRATION_LOCAL_ORG_SERVICE_TOKEN=synthetic-org-service-token-123456789 \
  INTEGRATION_LOCAL_BILLING_SERVICE_TOKEN=synthetic-billing-service-token-123456789 \
  INTEGRATION_LOCAL_AUDIT_SERVICE_TOKEN=synthetic-audit-service-token-123456789 \
  INTEGRATION_LOCAL_VAULT_KEY=synthetic-vault-key \
  INTEGRATION_LOCAL_GOOGLE_CLIENT_ID=synthetic-client-id \
  INTEGRATION_LOCAL_WRITE_ATTESTATION_KEYS_JSON='[]' \
  docker compose -f "$compose_file" -f "$root/docker-compose.integration-persistent.local.public.yml" config --format json |
  jq -e '
    .services."integration-api".environment.INTEGRATION_PUBLIC_BASE_URL == "http://localhost:3026"
    and .services."integration-api".ports[0].published == "3026"
    and .services."integration-api".ports[0].host_ip == "127.0.0.1"
  ' >/dev/null

echo "persistent Integration Core compose contract passed"

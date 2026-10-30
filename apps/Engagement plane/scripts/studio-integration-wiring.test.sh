#!/bin/sh
# Verify deployment wiring without reading project secrets or contacting providers.
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
export STUDIO_SOCIAL_INTERNAL_API_KEY=synthetic-social-key
export STUDIO_LEADS_INTERNAL_API_KEY=synthetic-leads-key
export STUDIO_MARKETING_INTERNAL_API_KEY=synthetic-marketing-key
export STUDIO_SOCIAL_NATS_PASSWORD=synthetic-social-nats
export STUDIO_LEADS_NATS_PASSWORD=synthetic-leads-nats
export STUDIO_PLANE_DB_PASSWORD=synthetic-db-password
export STUDIO_PLANE_DRAGONFLY_PASSWORD=synthetic-cache-password
export STUDIO_NATS_PROVISIONER_PASSWORD=synthetic-provisioner-password
export GRAFANA_ADMIN_PASSWORD=synthetic-grafana-password
export STUDIO_SOCIAL_INTEGRATION_BEARER_FILE=/tmp/synthetic-social-bearer
export STUDIO_LEADS_INTEGRATION_BEARER_FILE=/tmp/synthetic-leads-bearer
export STUDIO_SOCIAL_INTEGRATION_ORGANIZATION_ID=synthetic-org
export STUDIO_LEADS_INTEGRATION_ORGANIZATION_ID=synthetic-org

docker compose -f "$root/docker-compose.yml" --profile marketing config --format json |
  jq -e '
    [.services."social-core", .services."leads-core", .services."marketing-core"] |
    all(.[]; .environment.INTEGRATION_CORE_URL == "http://integration-api:3026"
      and (.networks | has("inter-plane-bus"))
      and .environment.GOOGLE_CLIENT_SECRET == null
      and .environment.GOOGLE_CLIENT_ID == null
      and .environment.META_APP_SECRET == null)
  ' >/dev/null

for core in social leads; do
  docker compose -f "$root/docker-compose.yml" -f "$root/docker-compose.$core-integrations.studio.yml" \
    --profile marketing config --format json |
    jq -e --arg service "$core-core" '
      .services[$service] |
      .environment.INTEGRATION_CORE_TENANT_BEARER_TOKEN_FILE != null
      and .environment.INTEGRATION_CORE_TENANT_ORGANIZATION_ID != ""
      and (.secrets | length) == 1
      and .environment.SOCIAL_PUBLISH_WORKER_ENABLED != "true"
      and .environment.PROVIDER_LEAD_SYNC_ENABLED != "true"
    ' >/dev/null
done

echo "shared Studio Integration wiring contract passed"

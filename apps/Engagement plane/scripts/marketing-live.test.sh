#!/bin/sh
# Render the live path using synthetic identifiers; no OAuth or token calls.
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
export STUDIO_MARKETING_INTEGRATION_ORGANIZATION_ID=synthetic-org
export STUDIO_MARKETING_GOOGLE_CONNECTION_ID=synthetic-google-connection
export STUDIO_MARKETING_SERVICE_KEY_FILE=/tmp/synthetic-service-key

docker compose -f "$root/docker-compose.yml" \
  -f "$root/docker-compose.marketing-bearer.studio.yml" \
  -f "$root/docker-compose.marketing-live.studio.yml" \
  --profile marketing config --format json |
  jq -e '
    .services."marketing-bearer-refresh".environment.AUTH_CORE_URL == "http://auth-core:3011"
    and .services."marketing-bearer-refresh".environment.MARKETING_INTEGRATION_ORGANIZATION_ID == "synthetic-org"
    and .services."marketing-bearer-refresh".environment.MARKETING_INTEGRATION_SERVICE_KEY_FILE == "/run/secrets/marketing_plane_service_key"
    and .services."marketing-core".environment.GOOGLE_CONNECTION_ID == "synthetic-google-connection"
    and .services."marketing-core".environment.INTEGRATION_BEARER_TOKEN_FILE == "/run/marketing-bearer/token"
    and .services."marketing-core".environment.MARKETING_ORGANIZATION_ID == "synthetic-org"
    and .services."marketing-core".environment.MARKETING_WEBSITES_FILE == "/var/lib/marketing/websites.json"
    and (.services."marketing-core".volumes | any(.target == "/var/lib/marketing" and .read_only == null))
    and .services."marketing-websites-init".network_mode == "none"
    and .services."marketing-core".environment.MARKETING_INTEGRATION_SERVICE_KEY_FILE == null
    and .services."marketing-core".secrets == null
    and (.services."marketing-core".volumes | any(.target == "/run/marketing-bearer" and .read_only == true))
    and (.services."marketing-bearer-refresh".volumes | any(.target == "/run/marketing-bearer" and .read_only == null))
    and .services."marketing-core".depends_on."marketing-bearer-refresh".condition == "service_healthy"
    and .services."marketing-bearer-refresh".healthcheck.test[1] == "/app/integration-bearer-refresh"
    and .services."marketing-bearer-init".network_mode == "none"
  ' >/dev/null
echo "live Marketing bearer deployment contract passed"

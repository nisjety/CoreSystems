#!/bin/sh
set -eu
root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
export STUDIO_SOCIAL_INTERNAL_API_KEY=synthetic-social STUDIO_LEADS_INTERNAL_API_KEY=synthetic-leads STUDIO_MARKETING_INTERNAL_API_KEY=synthetic-marketing
export STUDIO_SOCIAL_NATS_PASSWORD=synthetic-social STUDIO_LEADS_NATS_PASSWORD=synthetic-leads STUDIO_NATS_PROVISIONER_PASSWORD=synthetic-provisioner
export STUDIO_PLANE_DB_PASSWORD=synthetic-db STUDIO_PLANE_DRAGONFLY_PASSWORD=synthetic-cache GRAFANA_ADMIN_PASSWORD=synthetic-grafana
export STUDIO_ANBUD_INTERNAL_API_KEY=synthetic-anbud STUDIO_ANBUD_DB_PASSWORD=synthetic-runtime STUDIO_ANBUD_MIGRATOR_DB_PASSWORD=synthetic-owner
docker compose -f "$root/docker-compose.yml" --profile anbud config --format json |
 jq -e '
  .services."anbud-core" as $api | .services."anbud-migrate" as $migration |
  $api.environment.ANBUD_SKIP_MIGRATIONS == "true" and
  $api.environment.ANBUD_MIGRATE_DSN == "" and
  ($api.environment.ANBUD_DSN | contains("anbud_app:synthetic-runtime@engagement-postgres:5432/anbud_core")) and
  $api.environment.ANBUD_API_KEY == "synthetic-anbud" and
  $api.environment.ANBUD_API_KEY_HEADER == "X-Internal-API-Key" and
  $api.depends_on."anbud-migrate".condition == "service_completed_successfully" and
  $api.read_only == true and ($api.networks | has("inter-plane-bus")) and
  ($api.healthcheck.test | join(" ") | contains("/ready")) and
  ($migration.command | index("--migrate-only") != null) and
  $migration.environment.ANBUD_DSN == null and
  $migration.environment.ANBUD_API_KEY == null and
  ($migration.environment.ANBUD_MIGRATE_DSN | contains("anbud_migrator:synthetic-owner@engagement-postgres:5432/anbud_core")) and
  $migration.restart == "no" and ($migration.networks | has("inter-plane-bus") | not)
 ' >/dev/null
echo 'canonical Anbud runtime/migration wiring passed'
